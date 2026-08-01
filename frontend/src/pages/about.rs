use leptos::prelude::*;

use crate::components::style::{CLASS_PAGE_TITLE, CLASS_TEXT_MUTED};
use crate::components::use_page_title;
use crate::t;

const PROJECT_URL: &str = "https://github.com/honbey/ait";
const PROJECT_LICENSE: &str = "MIT";

#[component]
pub fn AboutPage() -> impl IntoView {
    use_page_title(move || format!("Ait - {}", t!(About)()));
    view! {
        <div class="min-h-[calc(100vh-3.5rem)] bg-gray-50 dark:bg-gray-900">
            <div class="max-w-2xl mx-auto px-6 py-8">
                <h1 class=CLASS_PAGE_TITLE>{t!(About)}</h1>
                <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm">
                    <div class="flex items-center gap-4 p-6 border-b border-gray-100 dark:border-gray-700">
                        <img src="/ait-logo.svg" alt="Ait" class="h-12 w-12" />
                        <div>
                            <h2 class="text-xl font-bold text-gray-900 dark:text-gray-100">Ait</h2>
                            <p class=format!("text-sm {}", CLASS_TEXT_MUTED)>{t!(AboutDesc)}</p>
                        </div>
                    </div>
                    <div class="p-6 space-y-4">
                        <div class="flex items-center justify-between">
                            <span class=format!("text-sm {}", CLASS_TEXT_MUTED)>{t!(Version)}</span>
                            <span class="text-sm font-medium text-gray-900 dark:text-gray-100 font-mono">
                                {env!("CARGO_PKG_VERSION")}
                            </span>
                        </div>
                        <div class="flex items-center justify-between">
                            <span class=format!("text-sm {}", CLASS_TEXT_MUTED)>{t!(License)}</span>
                            <span class="text-sm font-medium text-gray-900 dark:text-gray-100">
                                {PROJECT_LICENSE}
                            </span>
                        </div>
                        <div class="flex items-center justify-between">
                            <span class=format!("text-sm {}", CLASS_TEXT_MUTED)>{t!(Github)}</span>
                            <a
                                href=PROJECT_URL
                                target="_blank"
                                rel="noopener noreferrer"
                                class="text-sm font-medium text-indigo-600 dark:text-indigo-400 \
                                hover:underline flex items-center gap-1"
                            >
                                "github.com/honbey/ait"
                                <i class="fas fa-arrow-up-right-from-square text-xs"></i>
                            </a>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
