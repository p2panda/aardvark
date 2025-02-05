/* textbuffer.rs
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

use std::cell::{Cell, OnceCell, RefCell};

use aardvark_doc::document::Document;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, glib::clone};
use sourceview::prelude::BufferExt;
use sourceview::subclass::prelude::*;
use sourceview::*;
use tracing::{error, info};

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::AardvarkTextBuffer)]
    pub struct AardvarkTextBuffer {
        pub inhibit_text_change: Cell<bool>,
        pub document_handlers: OnceCell<glib::SignalGroup>,
        #[property(get, set = Self::set_document)]
        pub document: RefCell<Option<Document>>,
    }

    impl AardvarkTextBuffer {
        fn set_document(&self, document: Option<&Document>) {
            if let Some(document) = document.as_ref() {
                self.obj().set_inhibit_text_change(true);
                self.obj().set_text(&document.text());
                self.obj().set_inhibit_text_change(false);
            }

            self.document_handlers.get().unwrap().set_target(document);
            self.document.replace(document.cloned());
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkTextBuffer {
        const NAME: &'static str = "AardvarkTextBuffer";
        type Type = super::AardvarkTextBuffer;
        type ParentType = sourceview::Buffer;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AardvarkTextBuffer {
        fn constructed(&self) {
            let manager = adw::StyleManager::default();
            let buffer = self.obj();

            let language_manager = sourceview::LanguageManager::new();
            let markdown = language_manager.language("markdown");

            buffer.set_language(markdown.as_ref());
            // FIXME: When using subclassing highlight matching brackets causes a crash
            // See: https://gitlab.gnome.org/World/Rust/sourceview5-rs/-/issues/11
            buffer.set_highlight_matching_brackets(false);
            buffer.set_style_scheme(style_scheme().as_ref());

            manager.connect_dark_notify(glib::clone!(
                #[weak]
                buffer,
                move |_| {
                    buffer.set_style_scheme(style_scheme().as_ref());
                }
            ));

            // We could use a signal group to block hanlders
            let document_handlers = glib::SignalGroup::with_type(Document::static_type());
            document_handlers.connect_local(
                "text-inserted",
                false,
                clone!(
                    #[weak]
                    buffer,
                    #[upgrade_or]
                    None,
                    move |values| {
                        let pos: i32 = values.get(1).unwrap().get().unwrap();
                        let text: &str = values.get(2).unwrap().get().unwrap();

                        let mut pos_iter = buffer.iter_at_offset(pos);
                        buffer.set_inhibit_text_change(true);
                        buffer.insert(&mut pos_iter, text);
                        buffer.set_inhibit_text_change(false);

                        None
                    }
                ),
            );

            document_handlers.connect_local(
                "range-deleted",
                false,
                clone!(
                    #[weak]
                    buffer,
                    #[upgrade_or]
                    None,
                    move |values| {
                        let start: i32 = values.get(1).unwrap().get().unwrap();
                        let end: i32 = values.get(2).unwrap().get().unwrap();
                        let mut start = buffer.iter_at_offset(start);
                        let mut end = buffer.iter_at_offset(end);
                        buffer.set_inhibit_text_change(true);
                        buffer.delete(&mut start, &mut end);
                        buffer.set_inhibit_text_change(false);

                        None
                    }
                ),
            );

            self.document_handlers.set(document_handlers).unwrap();
        }
    }

    impl TextBufferImpl for AardvarkTextBuffer {
        fn insert_text(&self, iter: &mut gtk::TextIter, new_text: &str) {
            let offset = iter.offset();
            info!("inserting new text {} at pos {}", new_text, offset);

            if !self.inhibit_text_change.get() {
                if let Some(document) = self.document.borrow().as_ref() {
                    self.document_handlers.get().unwrap().block();
                    if let Err(error) = document.insert_text(offset, new_text) {
                        error!("Failed to submit changes to the document: {error}");
                    }
                    self.document_handlers.get().unwrap().unblock();
                }
            }

            self.parent_insert_text(iter, new_text);
        }

        fn delete_range(&self, start: &mut gtk::TextIter, end: &mut gtk::TextIter) {
            let offset_start = start.offset();
            let offset_end = end.offset();
            info!(
                "deleting range at start {} end {}",
                offset_start, offset_end
            );

            if !self.inhibit_text_change.get() {
                if let Some(document) = self.document.borrow().as_ref() {
                    self.document_handlers.get().unwrap().block();
                    if let Err(error) = document.delete_range(offset_start, offset_end) {
                        error!("Failed to submit changes to the document: {error}")
                    }
                    self.document_handlers.get().unwrap().unblock();
                }
            }

            self.parent_delete_range(start, end);
        }
    }

    impl BufferImpl for AardvarkTextBuffer {}
}

glib::wrapper! {
    pub struct AardvarkTextBuffer(ObjectSubclass<imp::AardvarkTextBuffer>)
        @extends gtk::TextBuffer, sourceview::Buffer;
}

impl AardvarkTextBuffer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    fn set_inhibit_text_change(&self, inhibit_text_change: bool) {
        self.imp().inhibit_text_change.set(inhibit_text_change);
    }

    pub fn full_text(&self) -> String {
        self.text(&self.start_iter(), &self.end_iter(), true).into()
    }
}

fn style_scheme() -> Option<sourceview::StyleScheme> {
    let manager = adw::StyleManager::default();
    let scheme_name = if manager.is_dark() {
        "Adwaita-dark"
    } else {
        "Adwaita"
    };

    sourceview::StyleSchemeManager::default().scheme(scheme_name)
}
