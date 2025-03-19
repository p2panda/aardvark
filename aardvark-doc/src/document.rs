use std::cell::{Cell, OnceCell};
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use aardvark_node::document::{DocumentId as DocumentIdNode, SubscribableDocument};
use anyhow::Result;
use glib::prelude::*;
use glib::subclass::{Signal, prelude::*};
use glib::{Properties, clone};
use p2panda_core::{HashError, PublicKey};
use tracing::error;

use crate::crdt::{TextCrdt, TextCrdtEvent, TextDelta};
use crate::service::Service;

#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "AardvarkDocumentId", nullable)]
pub struct DocumentId(DocumentIdNode);

impl FromStr for DocumentId {
    type Err = HashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(DocumentId(DocumentIdNode::from_str(value)?))
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Document)]
    pub struct Document {
        #[property(name = "text", get = Self::text, type = String)]
        crdt_doc: OnceCell<TextCrdt>,
        #[property(get, construct_only, set = Self::set_id)]
        id: OnceCell<DocumentId>,
        #[property(get, set)]
        ready: Cell<bool>,
        #[property(get, construct_only)]
        service: OnceCell<Service>,
        subscription_handle: OnceCell<glib::JoinHandle<()>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Document {
        const NAME: &'static str = "Document";
        type Type = super::Document;
    }

    impl Document {
        pub fn text(&self) -> String {
            self.crdt_doc.get().expect("crdt_doc to be set").to_string()
        }

        fn set_id(&self, id: Option<DocumentId>) {
            if let Some(id) = id {
                self.id.set(id).expect("Document id can only be set once");
            }
        }

        pub fn splice_text(&self, index: i32, delete_len: i32, chunk: &str) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            if delete_len == 0 {
                doc.insert(index as usize, chunk)
                    .expect("update document after text insertion");
            } else {
                doc.remove(index as usize, delete_len as usize)
                    .expect("update document after text removal");
            }

            Ok(())
        }

        pub fn on_remote_message(&self, bytes: &[u8]) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            if let Err(err) = doc.apply_encoded_delta(&bytes) {
                eprintln!("received invalid message: {}", err);
            }
        }

        fn emit_text_inserted(&self, pos: i32, text: String) {
            // Emit the signal on the main thread
            let obj = self.obj();
            glib::source::idle_add_full(
                glib::source::Priority::DEFAULT,
                clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        obj.emit_by_name::<()>("text-inserted", &[&pos, &text]);
                        glib::ControlFlow::Break
                    }
                ),
            );
        }

        fn emit_range_deleted(&self, start: i32, end: i32) {
            // Emit the signal on the main thread
            let obj = self.obj();
            glib::source::idle_add_full(
                glib::source::Priority::DEFAULT,
                clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        obj.emit_by_name::<()>("range-deleted", &[&start, &end]);
                        glib::ControlFlow::Break
                    }
                ),
            );
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Document {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("text-inserted")
                        .param_types([glib::types::Type::I32, glib::types::Type::STRING])
                        .build(),
                    Signal::builder("range-deleted")
                        .param_types([glib::types::Type::I32, glib::types::Type::I32])
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();

            if self.id.get().is_none() {
                let document_id = self
                    .obj()
                    .service()
                    .node()
                    .create_document()
                    .expect("Create document");
                self.set_id(Some(DocumentId(document_id)));
            }

            let public_key = self.obj().service().public_key();
            let crdt_doc = TextCrdt::new({
                // Take first 8 bytes of public key (32 bytes) to determine a unique "peer id"
                // which is used to keep authors apart inside the text crdt.
                //
                // TODO(adz): This is strictly speaking not collision-resistant but we're limited
                // here by the 8 bytes / 64 bit from the u64 `PeerId` type from Loro. In practice
                // this should not really be a problem, but it would be nice if the Loro API would
                // change some day.
                let mut buf = [0u8; 8];
                buf[..8].copy_from_slice(&public_key.as_bytes()[..8]);
                u64::from_be_bytes(buf)
            });

            let crdt_doc_rx = crdt_doc.subscribe();
            self.crdt_doc.set(crdt_doc).expect("crdt_doc not to be set");

            let document_id = self.obj().id().0;
            let node = self.obj().service().node().clone();
            let handle = DocumentHandle(self.obj().downgrade());
            let handle = glib::spawn_future(async move {
                if let Err(error) = node.subscribe(document_id, &handle).await {
                    error!("Failed to subscribe to document: {}", error);
                }
            });

            self.subscription_handle.set(handle).unwrap();

            let obj = self.obj();
            glib::spawn_future(clone!(
                #[weak]
                obj,
                async move {
                    while let Ok(event) = crdt_doc_rx.recv().await {
                        match event {
                            TextCrdtEvent::LocalEncoded(delta_bytes) => {
                                // Broadcast a "text delta" to all peers and persist the snapshot.
                                //
                                // TODO(adz): We should consider persisting the snapshot every x
                                // times or x seconds, not sure yet what logic makes the most
                                // sense.
                                let snapshot_bytes = obj
                                    .imp()
                                    .crdt_doc
                                    .get()
                                    .expect("crdt_doc to be set")
                                    .snapshot();

                                if obj
                                    .service()
                                    .node()
                                    .delta_with_snapshot(obj.id().0, delta_bytes, snapshot_bytes)
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            TextCrdtEvent::Local(text_deltas)
                            | TextCrdtEvent::Remote(text_deltas) => {
                                for delta in text_deltas {
                                    match delta {
                                        TextDelta::Insert { index, chunk } => {
                                            obj.imp().emit_text_inserted(index as i32, chunk);
                                        }
                                        TextDelta::Remove { index, len } => {
                                            obj.imp().emit_range_deleted(
                                                index as i32,
                                                (index + len) as i32,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            ));
        }

        fn dispose(&self) {
            if let Some(handle) = self.subscription_handle.get() {
                handle.abort();
            }
        }
    }
}

glib::wrapper! {
    pub struct Document(ObjectSubclass<imp::Document>);
}
impl Document {
    pub fn new(service: &Service, id: Option<&DocumentId>) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
            .build()
    }

    pub fn insert_text(&self, index: i32, chunk: &str) -> Result<()> {
        self.imp().splice_text(index, 0, chunk)
    }

    pub fn delete_range(&self, index: i32, end: i32) -> Result<()> {
        self.imp().splice_text(index, end - index, "")
    }
}

unsafe impl Send for Document {}
unsafe impl Sync for Document {}

struct DocumentHandle(glib::WeakRef<Document>);

impl SubscribableDocument for DocumentHandle {
    fn bytes_received(&self, _author: PublicKey, data: &[u8]) {
        if let Some(document) = self.0.upgrade() {
            document.imp().on_remote_message(data);
        }
    }
}
