use leptos::prelude::*;
use leptos_router::components::A;

use crate::auth::AuthContext;
use crate::components::skeleton::render_loading;
use crate::components::style::CLASS_TEXT_MUTED;
use crate::components::use_page_title;
use crate::t;

#[component]
pub fn Home() -> impl IntoView {
    let auth = use_context::<AuthContext>().expect("AuthContext not provided");
    use_page_title("Ait");

    move || {
        match auth.authenticated.get() {
        None => render_loading().into_any(),
        Some(_) => view! {
            <div class="min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900">
                <div class="text-center px-4">
                    <h1 class="text-6xl font-bold text-gray-900 dark:text-gray-100 mb-6">
                        {t!(IndexTitle)}
                    </h1>
                    <p class=format!(
                        "text-xl {} max-w-2xl mx-auto",
                        CLASS_TEXT_MUTED,
                    )>{t!(IndexSubtitle)}</p>
                    <div class="mt-8 flex items-center justify-center gap-4">
                        <Show
                            when=move || auth.authenticated.get() == Some(true)
                            fallback=|| {
                                view! {
                                    <A
                                        href="/login"
                                        {..}
                                        class="px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg cursor-pointer transition-colors inline-block active:scale-95"
                                    >
                                        {t!(Login)}
                                    </A>
                                }
                            }
                        >
                            <A
                                href="/console"
                                {..}
                                class="px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg cursor-pointer transition-colors inline-block active:scale-95"
                            >
                                {t!(Console)}
                            </A>
                        </Show>
                    </div>
                </div>
            </div>
        }.into_any(),
        }
    }
}
