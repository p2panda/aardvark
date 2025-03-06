/* window.rs
 *
 * Copyright 2024 The Aardvark Developers
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

use std::cell::RefCell;

use adw::prelude::ActionRowExt;
use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, glib::clone};

use aardvark_doc::authors::Authors;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ConnectionPopover)]
    pub struct ConnectionPopover {
        author_list_box: gtk::ListBox,
        #[property(get, set = Self::set_model)]
        model: RefCell<Option<Authors>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ConnectionPopover {
        const NAME: &'static str = "AardvarkConnectionPopover";
        type Type = super::ConnectionPopover;
        type ParentType = gtk::Popover;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ConnectionPopover {
        fn constructed(&self) {
            self.obj().set_child(Some(&self.author_list_box));
            self.author_list_box.set_selection_mode(gtk::SelectionMode::None);
        }
    }

    impl ConnectionPopover {
        fn set_model(&self, model: Option<Authors>) {
            self.author_list_box.bind_model(model.as_ref(), |author| {
                let row = adw::ActionRow::new();
                let avatar = adw::Avatar::new(64, None, true);
                row.add_prefix(&avatar);
                author.bind_property ("name", &row, "title").sync_create().build();
                // FIXME: format last seen according to the mockups
                //author.bind_property ("last-seen", row, "subtitle").sync_create().build();
                author.bind_property ("emoji", &avatar, "text").sync_create().build();

                row.upcast()
            });

            self.model.replace(model);
        }
    }

    impl WidgetImpl for ConnectionPopover {}
    impl PopoverImpl for ConnectionPopover {}
}

glib::wrapper! {
    pub struct ConnectionPopover(ObjectSubclass<imp::ConnectionPopover>)
        @extends gtk::Widget, gtk::Popover;
}

impl ConnectionPopover {
    pub fn new<P: IsA<Authors>>(model: &P) -> Self {
        glib::Object::builder()
            .property("model", model)
            .build()
    }
}
