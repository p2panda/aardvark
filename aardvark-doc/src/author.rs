use std::cell::OnceCell;

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::Properties;
use p2panda_core::Hash;
use emojis::Emoji;

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Author)]
    pub struct Author {
        #[property(name = "name", get = Self::name, type = String)]
        #[property(name = "emoji", get = Self::emoji, type = String)]
        pub emoji:   OnceCell<&'static Emoji>,
        pub public_key: OnceCell<Hash>,
        //last_seen: RefCell<String>,
        //status: Cell<Status>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Author {
        const NAME: &'static str = "Author";
        type Type = super::Author;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Author {}

    impl Author {
        fn name(&self) -> String {
            // TODO: The returned name should be title case
            self.emoji.get().unwrap().name().to_owned()
        }

        fn emoji(&self) -> String {
            // TODO: The returned name should be title case
            self.emoji.get().unwrap().as_str().to_owned()
        }
    }
}

glib::wrapper! {
    pub struct Author(ObjectSubclass<imp::Author>);
}
impl Author {
    pub fn new(public_key: Hash, emoji: &'static Emoji) -> Self {
        let obj: Self = glib::Object::new();

        obj.imp().public_key.set(public_key).unwrap();
        obj.imp().emoji.set(emoji).unwrap();
        obj
    }
}
