use leptos::prelude::*;
use leptos_router::{
    StaticSegment,
    components::{ParentRoute, Route, Router, Routes},
};

use crate::api;
use crate::auth::AuthContext;
use crate::components::console_layout::ConsoleShell;
use crate::components::toast::{ToastContainer, ToastManager};
use crate::components::topbar::Topbar;
use crate::i18n::I18n;
use crate::pages::apikeys::ApiKeysPage;
use crate::pages::home::Home;
use crate::pages::login::LoginPage;
use crate::pages::logs::LogsPage;
use crate::pages::models::ModelsPage;
use crate::pages::not_found::NotFoundPage;
use crate::pages::overview::Overview;
use crate::pages::providers::ProvidersPage;
use crate::pages::text_gen::TextGenPage;
use crate::storage;

#[component]
pub fn App() -> impl IntoView {
    let initial_lang = storage::get_item(storage::LANG_KEY).unwrap_or_else(|| "zh".to_string());
    let i18n = I18n::new(&initial_lang);
    provide_context(i18n.clone());

    let auth = AuthContext::new();
    provide_context(auth.clone());
    provide_context(ToastManager::new());
    let _session_check = LocalResource::new(move || {
        let auth_clone = auth.clone();
        async move {
            match api::check_session().await {
                Ok(Some(uname)) => auth_clone.set_logged_in(uname),
                Ok(None) => auth_clone.set_logged_out(),
                Err(_) => auth_clone.authenticated.set(Some(false)),
            }
        }
    });

    let initial_dark = matches!(
        storage::get_item(storage::THEME_KEY).as_deref(),
        Some("dark")
    );
    let dark = RwSignal::new(initial_dark);
    provide_context(dark);

    let i18n_clone = i18n.clone();
    Effect::new(move |_| {
        storage::set_item(
            storage::THEME_KEY,
            if dark.get() { "dark" } else { "light" },
        );
        storage::set_item(storage::LANG_KEY, &i18n_clone.lang());

        // Sync <html lang> with current language
        let lang = i18n_clone.lang();
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(html) = doc.document_element()
        {
            let _ = html.set_attribute("lang", if lang == "zh" { "zh-CN" } else { "en" });
        }
    });

    view! {
        <div class=move || if dark.get() { "dark" } else { "" }>
            <Router>
                <Topbar dark=dark />
                <main class="min-h-[calc(100vh-3.5rem)] animate-fadeIn">
                    <Routes fallback=|| view! { <NotFoundPage /> }>
                        <Route path=StaticSegment("") view=Home />
                        <Route path=StaticSegment("login") view=LoginPage />
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
