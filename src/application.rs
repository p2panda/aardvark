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
use automerge::ReadDoc;
use automerge::{AutoCommit, ObjId, ObjType};
use gettextrs::gettext;
use gtk::{gio, glib};
use tokio::sync::{mpsc, oneshot};

use crate::config::VERSION;
use crate::glib::closure_local;
use crate::network;
use crate::AardvarkWindow;

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct AardvarkApplication {
        window: OnceCell<AardvarkWindow>,
        automerge: RefCell<AutoCommit>,
        root: ObjId,
        #[allow(dead_code)]
        backend_shutdown_tx: oneshot::Sender<()>,
        tx: mpsc::Sender<Vec<u8>>,
        rx: RefCell<Option<mpsc::Receiver<Vec<u8>>>>,
    }

    impl AardvarkApplication {
        fn update_text(&self, text: &str) {
            println!("app: {}", text);
            let mut doc = self.automerge.borrow_mut();
            let current_text = doc.text(&self.root).unwrap();
            if text == current_text { return; }

            doc.update_text(&self.root, text).unwrap();

            {
                let bytes = doc.save();
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
            let mut am = AutoCommit::new();
            let root = am
                .put_object(automerge::ROOT, "root", ObjType::Text)
                .unwrap();
            let automerge = RefCell::new(am);

            let (backend_shutdown_tx, tx, rx) = network::run().expect("running p2p backend");

            AardvarkApplication {
                automerge,
                root,
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
                    window.connect_closure(
                        "text-changed",
                        false,
                        closure_local!(|_window: AardvarkWindow, text: &str| {
                            app.imp().update_text(text);
                        }),
                    );

                    let mut rx = application.imp().rx.take().unwrap();
                    let w = window.clone();
                    let app = application.clone();
                    glib::spawn_future_local(async move {
                        while let Some(msg) = rx.recv().await {
                            //println!("got {:?}", msg);
                            let text = {
                                let mut doc_remote = AutoCommit::load(&msg).unwrap();
                                let mut new_doc = AutoCommit::new();
                                new_doc.merge(&mut doc_remote).unwrap();
                                new_doc.text(&app.imp().root).unwrap()
                            };
                            dbg!(&text);
                            w.set_text(&text);
                        }
                    });

                    window
                })
                .clone();

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
            .application_name("aardvark")
            .application_icon("org.p2panda.aardvark")
            .developer_name("Tobias")
            .version(VERSION)
            .developers(vec!["Tobias"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(&gettext("translator-credits"))
            .copyright("© 2024 Tobias")
            .build();

        about.present(Some(&window));
    }
}
