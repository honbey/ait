use leptos::prelude::*;
use leptos_router::{
    StaticSegment,
    components::{ParentRoute, Route, Router, Routes},
};

use crate::api;
use crate::auth::{AuthContext, AuthStatus};
use crate::components::console_layout::ConsoleShell;
use crate::components::toast::{ToastContainer, ToastManager};
use crate::components::topbar::Topbar;
use crate::i18n::I18n;
use crate::pages::about::AboutPage;
use crate::pages::apikeys::ApiKeysPage;
use crate::pages::docs::DocsPage;
use crate::pages::home::Home;
use crate::pages::logs::LogsPage;
use crate::pages::models::ModelsPage;
use crate::pages::not_found::NotFoundPage;
use crate::pages::overview::Overview;
use crate::pages::providers::ProvidersPage;
use crate::pages::text_gen::TextGenPage;
use crate::storage;

fn prefers_dark_scheme() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .is_some_and(|mql| mql.matches())
}

fn system_lang() -> &'static str {
    match web_sys::window()
        .and_then(|w| w.navigator().language())
        .as_deref()
    {
        Some(l) if l.starts_with("zh") => "zh",
        Some(l) if l.starts_with("en") => "en",
        _ => "zh",
    }
}

#[component]
pub fn App() -> impl IntoView {
    let initial_lang =
        storage::get_item(storage::LANG_KEY).unwrap_or_else(|| system_lang().to_string());
    let i18n = I18n::new(&initial_lang);
    provide_context(i18n.clone());

    let auth = AuthContext::new();
    provide_context(auth.clone());
    provide_context(ToastManager::new());
    let _session_check = LocalResource::new(move || {
        let auth_clone = auth.clone();
        async move {
            match api::check_session().await {
                Ok(Some(uname)) => {
                    // Apply only while status is still Unknown; a login or
                    // logout that raced this request must not be overwritten
                    // by a stale snapshot.
                    if auth_clone.authenticated.get_untracked() == AuthStatus::Unknown {
                        auth_clone.set_logged_in(uname);
                    }
                }
                Ok(None) => {
                    if auth_clone.authenticated.get_untracked() == AuthStatus::Unknown {
                        auth_clone.set_logged_out();
                    }
                }
                // Network errors must not flip the status; the app may be
                // offline or the request may have raced a login.
                Err(_) => {}
            }
        }
    });

    let stored_theme = match storage::get_item(storage::THEME_KEY).as_deref() {
        Some("dark") => Some(true),
        Some("light") => Some(false),
        _ => None,
    };
    let theme = RwSignal::new(stored_theme);
    // Falls back to the OS preference once at startup; only an explicit
    // toggle writes the preference to storage.
    let dark = Memo::new(move |_| theme.get().unwrap_or_else(prefers_dark_scheme));
    provide_context(theme);
    provide_context(dark);

    let i18n_clone = i18n.clone();
    Effect::new(move |_| {
        // Sync <html lang> with current language
        let lang = i18n_clone.lang();
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(html) = doc.document_element()
        {
            let _ = html.set_attribute("lang", if lang == "zh" { "zh-CN" } else { "en" });
            // The dark class must live on <html> (not a child div) so the
            // body background picks it up via the .dark variant.
            if dark.get() {
                let _ = html.class_list().add_1("dark");
            } else {
                let _ = html.class_list().remove_1("dark");
            }
        }
    });

    view! {
        <div class=move || if dark.get() { "dark" } else { "" }>
            <Router>
                <Topbar dark=dark theme=theme />
                <main class="min-h-[calc(100vh-3.5rem)]">
                    <Routes fallback=|| view! { <NotFoundPage /> }>
                        <Route path=StaticSegment("") view=Home />
                        <Route path=StaticSegment("docs") view=DocsPage />
                        <Route path=StaticSegment("about") view=AboutPage />
                        <ParentRoute path=StaticSegment("console") view=ConsoleShell>
                            <Route path=StaticSegment("") view=Overview />
                            <Route path=StaticSegment("overview") view=Overview />
                            <Route path=StaticSegment("providers") view=ProvidersPage />
                            <Route path=StaticSegment("apikeys") view=ApiKeysPage />
                            <Route path=StaticSegment("logs") view=LogsPage />
                            <Route path=StaticSegment("models") view=ModelsPage />
                            <Route path=StaticSegment("text-generation") view=TextGenPage />
                        </ParentRoute>
                    </Routes>
                </main>
            </Router>
            <ToastContainer />
        </div>
    }
}
