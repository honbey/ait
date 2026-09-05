use leptos::prelude::*;

use crate::components::style::{CLASS_PAGE_TITLE, CLASS_TEXT_MUTED};
use crate::components::use_page_title;
use crate::i18n::{K, use_i18n};
use crate::t;

const PROJECT_README_URL: &str = "https://github.com/honbey/ait/blob/main/README.md";
const FRONTEND_README_URL: &str = "https://github.com/honbey/ait/blob/main/frontend/README.md";

#[component]
fn DocLinkCard(
    url: &'static str,
    icon_class: &'static str,
    title_key: K,
    description_key: K,
) -> impl IntoView {
    view! {
        <a
            href=url
            target="_blank"
            rel="noopener noreferrer"
            class="group bg-white dark:bg-ink-900 rounded-xl shadow-sm p-6 \
            border border-gray-100 dark:border-ink-700 \
            hover:shadow-md hover:border-indigo-300 dark:hover:border-indigo-700 \
            transition-all cursor-pointer"
        >
            <div class="flex items-center justify-between mb-3">
                <div class="flex items-center gap-3">
                    <i class=format!(
                        "{} w-8 text-2xl text-indigo-600 dark:text-indigo-400",
                        icon_class,
                    )></i>
                    <h2 class="text-lg font-semibold text-gray-900 dark:text-ink-100">
                        {move || use_i18n().t(title_key)}
                    </h2>
                </div>
                <i class="fas fa-arrow-up-right-from-square text-gray-400 dark:text-ink-500 \
                group-hover:text-indigo-500 transition-colors"></i>
            </div>
            <p class=format!(
                "text-sm {}",
                CLASS_TEXT_MUTED,
            )>{move || use_i18n().t(description_key)}</p>
        </a>
    }
}

#[component]
pub fn DocsPage() -> impl IntoView {
    use_page_title(move || format!("{} - Ait", t!(Docs)()));
    view! {
        <div class="min-h-[calc(100vh-3.5rem)] bg-gray-50 dark:bg-ink-950">
            <div class="max-w-4xl mx-auto px-6 py-8">
                <h1 class=CLASS_PAGE_TITLE>{t!(Docs)}</h1>
                <p class=format!("mb-8 {}", CLASS_TEXT_MUTED)>{t!(DocsDesc)}</p>
                <div class="grid gap-4 md:grid-cols-2">
                    <DocLinkCard
                        url=PROJECT_README_URL
                        icon_class="fas fa-book"
                        title_key=K::DocsProjectReadme
                        description_key=K::DocsProjectReadmeDesc
                    />
                    <DocLinkCard
                        url=FRONTEND_README_URL
                        icon_class="fas fa-code"
                        title_key=K::DocsFrontendReadme
                        description_key=K::DocsFrontendReadmeDesc
                    />
                </div>
            </div>
        </div>
    }
}
