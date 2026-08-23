use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::style::{CLASS_HERO_BTN, CLASS_TEXT_MUTED};
use crate::components::use_page_title;
use crate::t;

#[component]
pub fn NotFoundPage() -> impl IntoView {
    use_page_title(move || format!("{} - Ait", t!(PageNotFound)()));
    view! {
        <div class="min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900">
            <div class="text-center px-4">
                <h1 class="text-8xl font-bold text-gray-200 dark:text-gray-700 mb-4">404</h1>
                <p class=format!("text-xl {} mb-8", CLASS_TEXT_MUTED)>{t!(PageNotFound)}</p>
                <A href="/" {..} class=CLASS_HERO_BTN>
                    {t!(Index)}
                </A>
            </div>
        </div>
    }
}
