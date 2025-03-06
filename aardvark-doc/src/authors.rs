use std::cell::{Cell, OnceCell, RefCell};

use glib::prelude::*;
use glib::subclass::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use gio::prelude::*;
use p2panda_core::Hash;

use crate::author::Author;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Authors {
        pub list: RefCell<Vec<Author>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Authors {
        const NAME: &'static str = "Authors";
        type Type = super::Authors;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for Authors {}

    impl ListModelImpl for Authors {
        fn item_type(&self) -> glib::Type {
            Author::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, index: u32) -> Option<glib::Object> {
            self.list.borrow().get(index as usize).cloned().map(Cast::upcast)
        }
    }
}

glib::wrapper! {
    pub struct Authors(ObjectSubclass<imp::Authors>)
    @implements gio::ListModel;
}

impl Default for Authors {
    fn default() -> Self {
        Self::new()
    }
}

impl Authors {
    pub fn new() -> Self {
        let obj: Self = glib::Object::new();

                use rand::thread_rng;
        use rand::seq::IteratorRandom;
        //let emoji = emojis::Group::AnimalsAndNature.emojis().choose(&mut rand::thread_rng()).unwrap();
        //emojis::Group::AnimalsAndNature.emojis().for_each(|emoji| println!("Emoji {:?}", emoji));

        //obj.add_author(Author::new(Hash::new("random"), emoji));
         //       let emoji = emojis::Group::AnimalsAndNature.emojis().choose(&mut rand::thread_rng()).unwrap();
        //emojis::Group::AnimalsAndNature.emojis().for_each(|emoji| println!("Emoji {:?}", emoji));
           emojis::Group::AnimalsAndNature.emojis().take(10).for_each(|emoji| obj.add_author(Author::new(Hash::new("random"), emoji)));
        obj
    }

    pub(crate) fn add_author(&self, author: Author) {
        let mut list = self.imp().list.borrow_mut();
        let pos = list.len() as u32;

        list.push(author);
        drop(list);

        self.items_changed(pos, 0, 1)
    }
}
