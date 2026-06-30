use sycamore::prelude::*;
use sycamore_futures::spawn_local_scoped;
use sycamore_router::{HistoryIntegration, Router, RouterProps};

use crate::route::AppRoute;

mod api;
mod chart;
mod i18n;
mod layout;
mod models;
mod route;
mod storage;

mod components;
mod views;

use crate::components::toast::ToastManager;
use crate::i18n::K;
use crate::storage::{LANG_KEY, THEME_KEY, get_storage};

// ─── App Component ─────────────────────────────────────────────

#[component]
fn App() -> View {
    let storage = get_storage();
    let initial_dark = matches!(storage.get_item(THEME_KEY), Some(v) if v == "dark");
    let dark = create_signal(initial_dark);
    let authenticated = create_signal(None::<bool>);

    let initial_lang = storage
        .get_item(LANG_KEY)
        .unwrap_or_else(|| "zh".to_string());
    let i18n = i18n::I18n::new(&initial_lang);
    let toast_manager = ToastManager::new();

    let username = create_signal(None::<String>);

    // Register session-expired handler: on 401 from non-auth API calls
    let i18n_expired = i18n.clone();
    let toast_expired = toast_manager.clone();
    crate::api::set_session_expired_handler(Box::new(move || {
        authenticated.set(Some(false));
        toast_expired.error(i18n_expired.t(K::SessionExpired));
    }));

    // On mount, check if we have a valid session cookie
    spawn_local_scoped(async move {
        match crate::api::check_session().await {
            Ok(Some(uname)) => {
                username.set(Some(uname));
                authenticated.set(Some(true));
            }
            Ok(None) => authenticated.set(Some(false)),
            Err(_) => {} // network error — keep skeleton (authenticated stays None)
        }
    });

    let i18n_clone = i18n.clone();
    create_effect(move || {
        storage.set_item(LANG_KEY, &i18n_clone.lang());
        storage.set_item(THEME_KEY, if dark.get() { "dark" } else { "light" });
        let code = if i18n_clone.lang() == "zh" {
            "zh-CN"
        } else {
            "en"
        };
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(el) = doc.document_element()
        {
            let _ = el.set_attribute("lang", code);
        }
    });

    provide_context(i18n);
    provide_context(toast_manager);

    Router(RouterProps::new(
        HistoryIntegration::new(),
        move |route: ReadSignal<AppRoute>| {
            provide_context(route);
            layout::Layout(layout::LayoutProps {
                dark,
                authenticated,
                username,
            })
        },
    ))
}

fn main() {
    console_error_panic_hook::set_once();
    sycamore::render(App);
}
