use leptos::prelude::*;
use leptos_router::components::A;

use crate::auth::{AuthContext, AuthStatus};
use crate::components::style::{CLASS_NAV_LINK, CLASS_TEXT_MUTED};
use crate::i18n::I18n;
use crate::t;

#[component]
fn UserDropdown(auth: AuthContext, show_dropdown: RwSignal<bool>) -> impl IntoView {
    let uname = Memo::new(move |_| auth.username.get().unwrap_or_default());
    let avatar_letter = Memo::new(move |_| {
        uname
            .get()
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "U".to_string())
    });

    view! {
        <div class="relative">
            <div
                class="flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer transition-colors relative z-50"
                on:click=move |_| show_dropdown.set(!show_dropdown.get_untracked())
            >
                <div class="w-7 h-7 rounded-full bg-indigo-100 dark:bg-indigo-900 flex items-center justify-center text-indigo-600 dark:text-indigo-400 text-xs font-semibold">
                    {avatar_letter}
                </div>
                <span class="text-sm text-gray-700 dark:text-gray-300">{uname}</span>
                <i class="fas fa-chevron-down text-[10px] text-gray-400 dark:text-gray-500"></i>
            </div>
            <Show when=move || show_dropdown.get()>
                <div class="fixed inset-0 z-40" on:click=move |_| show_dropdown.set(false)></div>
                <div
                    class="absolute right-0 top-full mt-2 w-40 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <button
                        class="w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer active:scale-95 flex items-center gap-2"
                        on:click=move |_| {
                            show_dropdown.set(false);
                            web_sys::window().map(|w| w.location().set_href("/"));
                        }
                    >
                        <i class="fas fa-sign-out-alt w-4 text-center text-gray-400 dark:text-gray-500"></i>
                        <span>{t!(Logout)}</span>
                    </button>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn Topbar(dark: Memo<bool>, theme: RwSignal<Option<bool>>) -> impl IntoView {
    let i18n = use_context::<I18n>().expect("I18n");
    let auth = use_context::<AuthContext>().expect("AuthContext");

    let show_dropdown = RwSignal::new(false);

    view! {
        <nav class="bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 px-6 flex items-center justify-between h-14 sticky top-0 z-50">
            <div class="flex items-center gap-2">
                <A href="/" {..} class="flex items-center gap-2 mr-2 cursor-pointer">
                    <img src="/ait-logo.svg" alt="ait" class="h-8" />
                    <span class="text-xl font-bold text-gray-900 dark:text-gray-100">Ait</span>
                </A>
                <div class="flex items-center gap-2">
                    <A href="/" exact=true {..} class=CLASS_NAV_LINK>
                        <i class="fas fa-house w-4 text-center"></i>
                        <span>{t!(Index)}</span>
                    </A>
                    <A href="/console" {..} class=CLASS_NAV_LINK>
                        <i class="fas fa-terminal w-4 text-center"></i>
                        <span>{t!(Console)}</span>
                    </A>
                    <A href="/docs" {..} class=CLASS_NAV_LINK>
                        <i class="fas fa-book w-4 text-center"></i>
                        <span>{t!(Docs)}</span>
                    </A>
                    <A href="/about" {..} class=CLASS_NAV_LINK>
                        <i class="fas fa-info-circle w-4 text-center"></i>
                        <span>{t!(About)}</span>
                    </A>
                </div>
            </div>
            <div class="flex items-center gap-2">
                <button
                    class=format!(
                        "flex items-center gap-2 px-2 py-2 {} hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm active:scale-95",
                        CLASS_TEXT_MUTED,
                    )
                    on:click=move |_| {
                        let current = i18n.lang_untracked();
                        let new_lang = if current == "zh" { "en" } else { "zh" };
                        i18n.set_lang(new_lang);
                        crate::storage::set_item(crate::storage::LANG_KEY, new_lang);
                    }
                >
                    <i class="fas fa-globe w-4 text-center"></i>
                    <span>{t!(Language)}</span>
                </button>
                <button
                    class=format!(
                        "px-2 py-2 {} hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors active:scale-95",
                        CLASS_TEXT_MUTED,
                    )
                    on:click=move |_| {
                        let next = !dark.get_untracked();
                        theme.set(Some(next));
                        crate::storage::set_item(
                            crate::storage::THEME_KEY,
                            if next { "dark" } else { "light" },
                        );
                    }
                >
                    <i class=move || {
                        if dark.get() {
                            "fas fa-moon w-4 text-center"
                        } else {
                            "fas fa-sun w-4 text-center"
                        }
                    }></i>
                </button>
                <div class="w-px h-6 bg-gray-200 dark:bg-gray-700 mx-1"></div>

                <Show
                    when=move || auth.authenticated.get() == AuthStatus::Authenticated
                    fallback=|| {
                        view! {
                            <A
                                href="/console"
                                {..}
                                class="flex items-center gap-2 px-3 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer transition-colors text-sm text-gray-600 dark:text-gray-300"
                            >
                                <i class="fas fa-terminal w-4 text-center"></i>
                                <span>{t!(Console)}</span>
                            </A>
                        }
                    }
                >
                    <UserDropdown auth=auth.clone() show_dropdown=show_dropdown />
                </Show>
            </div>
        </nav>
    }
}
