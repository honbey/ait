use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gloo_storage::{LocalStorage, SessionStorage, Storage};

pub const THEME_KEY: &str = "ait-theme";
pub const LANG_KEY: &str = "ait-lang";

#[derive(Clone)]
enum Backend {
    Local,
    Session,
    Memory(Rc<RefCell<HashMap<String, String>>>),
}

fn probe<S: Storage>() -> bool {
    web_sys::window().is_some() && S::set("__gloo_probe__", true).is_ok() && {
        S::delete("__gloo_probe__");
        true
    }
}

fn probe_backend() -> Backend {
    if probe::<LocalStorage>() {
        Backend::Local
    } else if probe::<SessionStorage>() {
        Backend::Session
    } else {
        Backend::Memory(Rc::new(RefCell::new(HashMap::new())))
    }
}

thread_local! {
    static BACKEND: RefCell<Option<Backend>> = const { RefCell::new(None) };
}

fn backend() -> Backend {
    BACKEND.with(|b| {
        if b.borrow().is_none() {
            b.replace(Some(probe_backend()));
        }
        b.borrow().as_ref().unwrap().clone()
    })
}

pub fn get_item(key: &str) -> Option<String> {
    match backend() {
        Backend::Local => LocalStorage::get::<String>(key).ok(),
        Backend::Session => SessionStorage::get::<String>(key).ok(),
        Backend::Memory(map) => map.borrow().get(key).cloned(),
    }
}

pub fn set_item(key: &str, value: &str) {
    match backend() {
        Backend::Local => {
            if let Err(e) = LocalStorage::set(key, value.to_string()) {
                leptos::logging::error!("storage set_item '{}' failed: {:?}", key, e);
            }
        }
        Backend::Session => {
            if let Err(e) = SessionStorage::set(key, value.to_string()) {
                leptos::logging::error!("storage set_item '{}' failed: {:?}", key, e);
            }
        }
        Backend::Memory(map) => {
            map.borrow_mut().insert(key.to_string(), value.to_string());
        }
    }
}
