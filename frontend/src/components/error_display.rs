use leptos::prelude::*;

use crate::components::style::{CLASS_CARD, CLASS_FORM_ERROR};
use crate::t;

#[component]
pub fn ErrorCard(
    message: String,
    #[prop(optional)] on_retry: Option<Box<dyn Fn() + 'static>>,
) -> impl IntoView {
    view! {
        <div class=format!("{} p-8", CLASS_CARD)>
            <div class="text-center py-12">
                <p class="font-semibold text-gray-900 dark:text-gray-100 mb-2">{t!(LoadFailed)}</p>
                <p class="text-red-500 mb-4">{message}</p>
                {on_retry
                    .map(|cb| {
                        view! {
                            <button
                                class="px-4 py-2 bg-red-500 hover:bg-red-600 text-white rounded-lg transition-colors text-sm font-medium cursor-pointer active:scale-95"
                                on:click=move |_| (cb)()
                            >
                                {t!(Retry)}
                            </button>
                        }
                    })}
            </div>
        </div>
    }
}

#[component]
pub fn ErrorText(msg: RwSignal<String>) -> impl IntoView {
    view! {
        <Show when=move || !msg.get().is_empty()>
            <p class=CLASS_FORM_ERROR>{move || msg.get()}</p>
        </Show>
    }
}
