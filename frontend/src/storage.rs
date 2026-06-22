use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use sycamore::web::{console_error, window};

pub const THEME_KEY: &str = "ait-theme";
pub const LANG_KEY: &str = "ait-lang";

#[derive(Clone)]
pub enum AppStorage {
    Local(web_sys::Storage),
    Memory(Rc<RefCell<HashMap<String, String>>>),
}

impl AppStorage {
    pub fn get_item(&self, key: &str) -> Option<String> {
        match self {
            AppStorage::Local(s) => s.get_item(key).ok().flatten(),
            AppStorage::Memory(map) => map.borrow().get(key).cloned(),
        }
    }

    pub fn set_item(&self, key: &str, value: &str) {
        match self {
            AppStorage::Local(s) => {
                if let Err(e) = s.set_item(key, value) {
                    console_error!("localStorage set_item failed: {:?}", e);
                }
            }
            AppStorage::Memory(map) => {
                map.borrow_mut().insert(key.to_string(), value.to_string());
            }
        }
    }
}

pub fn get_storage() -> AppStorage {
    match window().local_storage() {
        Ok(Some(s)) => AppStorage::Local(s),
        _ => AppStorage::Memory(Rc::new(RefCell::new(HashMap::new()))),
    }
}
