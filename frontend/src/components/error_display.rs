use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen_futures::JsFuture;

use crate::components::style::{CLASS_CARD, CLASS_FORM_ERROR};
use crate::t;

/// Copy `value` to the system clipboard. Fire-and-forget: a clipboard the
/// browser refuses (no permission, insecure origin) must not break the error
/// card that is displaying the id.
fn copy_to_clipboard(value: String) {
    spawn_local(async move {
        let Some(window) = web_sys::window() else {
            return;
        };
        let promise = window.navigator().clipboard().write_text(&value);
        if JsFuture::from(promise).await.is_err() {
            leptos::logging::error!("clipboard write failed");
        }
    });
}

#[component]
pub fn ErrorCard(
    message: String,
    /// Correlation id from the failing response, shown so it can be matched
    /// against the server log.
    #[prop(optional_no_strip)]
    request_id: Option<String>,
    #[prop(optional)] on_retry: Option<Box<dyn Fn() + 'static>>,
) -> impl IntoView {
    view! {
        <div class=format!("{} p-8", CLASS_CARD)>
            <div class="text-center py-12">
                <p class="font-semibold text-gray-900 dark:text-ink-100 mb-2">{t!(LoadFailed)}</p>
                <p class="text-red-500 mb-4">{message}</p>
                {request_id
                    .map(|id| {
                        let copy_value = id.clone();
                        view! {
                            <div class="flex items-center justify-center gap-2 mb-4 text-xs text-gray-400 dark:text-ink-500">
                                <span>{t!(RequestId)}</span>
                                <code class="font-mono">{id}</code>
                                <button
                                    class="text-current opacity-50 hover:opacity-100 cursor-pointer transition-opacity active:scale-95"
                                    title=t!(Copy)
                                    on:click=move |_| copy_to_clipboard(copy_value.clone())
                                >
                                    <i class="fas fa-copy"></i>
                                </button>
                            </div>
                        }
                    })}
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
