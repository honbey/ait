use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::style::CLASS_TEXT_MUTED;
use crate::components::use_page_title;
use crate::t;

#[component]
pub fn Home() -> impl IntoView {
    use_page_title(move || format!("{} - Ait", t!(IndexTitle)()));

    move || {
        view! {
            <div class="min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-ink-950">
                <div class="text-center px-4">
                    <h1 class="text-6xl font-bold text-gray-900 dark:text-ink-100 mb-6">
                        {t!(IndexTitle)}
                    </h1>
                    <p class=format!(
                        "text-xl {} max-w-2xl mx-auto",
                        CLASS_TEXT_MUTED,
                    )>{t!(IndexSubtitle)}</p>
                    <div class="mt-8 flex items-center justify-center gap-4">
                        <A
                            href="/console"
                            {..}
                            class="px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg cursor-pointer transition-colors inline-block active:scale-95"
                        >
                            {t!(Console)}
                        </A>
                    </div>
                </div>
            </div>
        }
        .into_any()
    }
}
