use sycamore::prelude::*;
use sycamore_futures::spawn_local_scoped;
use sycamore_router::{HistoryIntegration, Router, RouterProps};

use crate::route::AppRoute;

mod api;
mod i18n;
mod layout;
mod models;
mod route;
mod storage;

mod components;
mod views;

use crate::storage::{LANG_KEY, THEME_KEY, get_storage};

// ─── App Component ─────────────────────────────────────────────

#[component]
fn App() -> View {
    let storage = get_storage();
    let initial_dark = matches!(storage.get_item(THEME_KEY), Some(v) if v == "dark");
    let dark = create_signal(initial_dark);
    let authenticated = create_signal(false);
    let session_checked = create_signal(false);

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
        session_checked.set(true);
    });

    let i18n_clone = i18n.clone();
    create_effect(move || {
        storage.set_item(LANG_KEY, &i18n_clone.lang());
        storage.set_item(THEME_KEY, if dark.get() { "dark" } else { "light" });
    });

    provide_context(i18n);

    Router(RouterProps::new(
        HistoryIntegration::new(),
        move |route: ReadSignal<AppRoute>| {
            provide_context(route);
            layout::Layout(layout::LayoutProps {
                dark,
                session_checked,
                authenticated,
                username,
                role,
            })
        },
    ))
}

fn main() {
    console_error_panic_hook::set_once();
    sycamore::render(App);
}
