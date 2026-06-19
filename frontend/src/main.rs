use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use sycamore::web::console_error;

use sycamore::prelude::*;
use sycamore_futures::spawn_local_scoped;

mod api;
mod i18n;
mod layout;
mod models;
mod route;

mod components;
mod views;

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

// ─── App Component ─────────────────────────────────────────────

#[component]
fn App() -> View {
    let storage = get_storage();
    let initial_dark = matches!(storage.get_item(THEME_KEY), Some(v) if v == "dark");
    let dark = create_signal(initial_dark);
    let route = create_signal(route::Route::Index);
    let authenticated = create_signal(false);

    let initial_lang = storage
        .get_item(LANG_KEY)
        .unwrap_or_else(|| "zh".to_string());
    let i18n = i18n::I18n::new(&initial_lang);

    let username = create_signal(None::<String>);
    let role = create_signal(None::<String>);

    // On mount, check if we have a valid session cookie
    spawn_local_scoped(async move {
        if let Ok(Some((uname, r))) = crate::api::check_session().await {
            username.set(Some(uname));
            role.set(Some(r));
            authenticated.set(true);
        }
    });

    let i18n_clone = i18n.clone();
    create_effect(move || {
        storage.set_item(LANG_KEY, &i18n_clone.lang());
        storage.set_item(THEME_KEY, if dark.get() { "dark" } else { "light" });
    });

    provide_context(i18n);

    layout::Layout(layout::LayoutProps {
        dark,
        route,
        authenticated,
        username,
        role,
    })
}

fn main() {
    console_error_panic_hook::set_once();
    sycamore::render(App);
}
