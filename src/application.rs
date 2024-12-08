/* application.rs
 *
 * Copyright 2024 Tobias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::cell::{OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use automerge::transaction::Transactable;
use automerge::PatchAction;
use automerge::ReadDoc;
use automerge::{AutoCommit, ObjType};
use gettextrs::gettext;
use gtk::{gio, glib};
use tokio::sync::{mpsc, oneshot};

use crate::config::VERSION;
use crate::glib::closure_local;
use crate::network;
use crate::{AardvarkTextBuffer, AardvarkWindow};

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct AardvarkApplication {
        window: OnceCell<AardvarkWindow>,
        automerge: RefCell<AutoCommit>,
        #[allow(dead_code)]
        backend_shutdown_tx: oneshot::Sender<()>,
        tx: mpsc::Sender<Vec<u8>>,
        rx: RefCell<Option<mpsc::Receiver<Vec<u8>>>>,
    }

    impl AardvarkApplication {
        fn update_text(&self, position: i32, del: i32, text: &str) {
            let mut doc = self.automerge.borrow_mut();

            let root = match doc.get(automerge::ROOT, "root").expect("root exists") {
                Some(root) => root.1,
                None => doc
                    .put_object(automerge::ROOT, "root", ObjType::Text)
                    .expect("inserting map at root"),
            };
            println!("root = {}", root);

            doc.splice_text(&root, position as usize, del as isize, text)
                .unwrap();

            // move the diff pointer forward to current position
            doc.update_diff_cursor();

            /*
            let patches = doc.diff_incremental();
            for patch in patches.iter() {
                println!("{}", patch.action);
                match &patch.action {
                    PatchAction::SpliceText { index: _, value: _, marks: _ } => {},
                    PatchAction::DeleteSeq { index: _, length: _ } => {},
                    PatchAction::PutMap { key: _, value: _, conflict: _ } => {},
                    PatchAction::PutSeq { index: _, value: _, conflict: _ } => {},
                    PatchAction::Insert { index: _, values: _ } => {},
                    PatchAction::Increment { prop: _, value: _ } => {},
                    PatchAction::Conflict { prop: _ } => {},
                    PatchAction::DeleteMap { key: _ } => {},
                    PatchAction::Mark { marks: _ } => {},
                }
            }
            */

            {
                let bytes = doc.save_incremental();
                let tx = self.tx.clone();
                glib::spawn_future_local(async move {
                    if let Err(e) = tx.send(bytes).await {
                        println!("{}", e);
                    }
                });
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkApplication {
        const NAME: &'static str = "AardvarkApplication";
        type Type = super::AardvarkApplication;
        type ParentType = adw::Application;

        fn new() -> Self {
            let automerge = RefCell::new(AutoCommit::new());
            let (backend_shutdown_tx, tx, rx) = network::run().expect("running p2p backend");

            AardvarkApplication {
                automerge,
                backend_shutdown_tx,
                tx,
                rx: RefCell::new(Some(rx)),
                window: OnceCell::new(),
            }
        }
    }

    impl ObjectImpl for AardvarkApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for AardvarkApplication {
        // We connect to the activate callback to create a window when the application
        // has been launched. Additionally, this callback notifies us when the user
        // tries to launch a "second instance" of the application. When they try
        // to do that, we'll just present any existing window.
        fn activate(&self) {
            let application = self.obj();
            // Get the current window or create one if necessary
            let window = self
                .window
                .get_or_init(|| {
                    let window = AardvarkWindow::new(&*application);
                    let app = application.clone();
                    let mut rx = application.imp().rx.take().unwrap();
                    let w = window.clone();

                    glib::spawn_future_local(async move {
                        while let Some(bytes) = rx.recv().await {
                            println!("got {:?}", bytes);
                            let text = {
                                let mut doc_local = app.imp().automerge.borrow_mut();
                                doc_local.load_incremental(&bytes).unwrap();
                                println!("LOCAL:");
                                print_document(&*doc_local);

                                let root = match doc_local
                                    .get(automerge::ROOT, "root")
                                    .expect("root exists")
                                {
                                    Some(root) => root.1,
                                    None => doc_local
                                        .put_object(automerge::ROOT, "root", ObjType::Text)
                                        .expect("inserting map at root"),
                                };
                                println!("root = {}", root);

                                // get the latest changes
                                let patches = doc_local.diff_incremental();
                                for patch in patches.iter() {
                                    println!("PATCH RECEIVED: {}", patch.action);
                                    match &patch.action {
                                        PatchAction::SpliceText {
                                            index,
                                            value,
                                            marks: _,
                                        } => {
                                            w.splice_text_view(
                                                *index as i32,
                                                0,
                                                value.make_string().as_str(),
                                            );
                                        }
                                        PatchAction::DeleteSeq { index, length } => {
                                            w.splice_text_view(*index as i32, *length as i32, "");
                                        }
                                        PatchAction::PutMap {
                                            key: _,
                                            value: _,
                                            conflict: _,
                                        } => {}
                                        PatchAction::PutSeq {
                                            index: _,
                                            value: _,
                                            conflict: _,
                                        } => {}
                                        PatchAction::Insert {
                                            index: _,
                                            values: _,
                                        } => {}
                                        PatchAction::Increment { prop: _, value: _ } => {}
                                        PatchAction::Conflict { prop: _ } => {}
                                        PatchAction::DeleteMap { key: _ } => {}
                                        PatchAction::Mark { marks: _ } => {}
                                    }
                                }

                                doc_local.text(&root).unwrap()
                            };
                            dbg!(&text);
                        }
                    });

                    window
                })
                .clone();

            let app = application.clone();
            window.clone().get_text_buffer().connect_closure(
                "text-change",
                false,
                closure_local!(|_buffer: AardvarkTextBuffer,
                                position: i32,
                                del: i32,
                                text: &str| {
                    app.imp().update_text(position, del, text);
                }),
            );

            // Ask the window manager/compositor to present the window
            window.upcast::<gtk::Window>().present();
        }
    }

    impl GtkApplicationImpl for AardvarkApplication {}
    impl AdwApplicationImpl for AardvarkApplication {}
}

glib::wrapper! {
    pub struct AardvarkApplication(ObjectSubclass<imp::AardvarkApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl AardvarkApplication {
    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build()
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        self.add_action_entries([quit_action, about_action]);
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let about = adw::AboutDialog::builder()
            .application_name("Aardvark")
            .application_icon("org.p2panda.aardvark")
            .developer_name("The Aardvark Developers")
            .version(VERSION)
            .developers(vec!["Tobias"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(&gettext("translator-credits"))
            .copyright("© 2024 Tobias")
            .build();

        about.present(Some(&window));
    }
}

fn print_document<R>(doc: &R)
where
    R: ReadDoc,
{
    let serialized = serde_json::to_string_pretty(&automerge::AutoSerde::from(doc)).unwrap();
    println!("{serialized}");
}
