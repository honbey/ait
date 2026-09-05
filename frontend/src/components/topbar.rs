use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::style::{CLASS_NAV_LINK, CLASS_TEXT_MUTED};
use crate::i18n::I18n;
use crate::t;

#[component]
pub fn Topbar(dark: Memo<bool>, theme: RwSignal<Option<bool>>) -> impl IntoView {
    let i18n = use_context::<I18n>().expect("I18n");

    view! {
        <nav class="bg-white dark:bg-ink-900 border-b border-gray-200 dark:border-ink-700 px-6 flex items-center justify-between h-14 sticky top-0 z-50">
            <div class="flex items-center gap-2">
                <A href="/" {..} class="flex items-center gap-2 mr-2 cursor-pointer">
                    <img src="/ait-logo.svg" alt="ait" class="h-8" />
                    <span class="text-xl font-bold text-gray-900 dark:text-ink-100">Ait</span>
                </A>
                <div class="flex items-center gap-2">
                    <A href="/" exact=true {..} class=CLASS_NAV_LINK>
                        <i class="fas fa-house w-4 text-center"></i>
                        <span>{t!(Index)}</span>
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
                <div class="w-px h-6 bg-gray-200 dark:bg-ink-800 mx-1"></div>

                <A
                    href="/console"
                    {..}
                    class="flex items-center gap-2 px-3 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-ink-900 cursor-pointer transition-colors text-sm text-gray-600 dark:text-ink-300"
                >
                    <i class="fas fa-terminal w-4 text-center"></i>
                    <span>{t!(Console)}</span>
                </A>
            </div>
        </nav>
    }
}
