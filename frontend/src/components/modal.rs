use leptos::ev;
use leptos::prelude::*;

use crate::components::style::{CLASS_BTN_CANCEL, CLASS_BTN_DANGER, CLASS_ICON_BTN};
use crate::components::toast::use_toast;
use crate::{t, tr, trs, ts};

/// One-shot modal shell. All props are static (not reactive) — modals are
/// dismissed and re-created on language switch, so tracking is unnecessary.
#[component]
pub fn ModalShell(
    on_close: impl Fn() + 'static + Clone + Send,
    title: String,
    #[prop(default = "max-w-md")] card_class: &'static str,
    children: Children,
) -> impl IntoView {
    let class = format!(
        "relative z-10 bg-white dark:bg-gray-800 rounded-xl p-6 shadow-2xl w-full mx-4 {}",
        card_class
    );
    let on_close_esc = on_close.clone();
    let close = move |_: leptos::ev::MouseEvent| on_close();
    let handle = window_event_listener(ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            on_close_esc();
        }
    });
    on_cleanup(move || handle.remove());
    view! {
        <div class="fixed inset-0 z-50 flex items-center justify-center">
            <div class="absolute inset-0 bg-black/50" on:click=close.clone()></div>
            <div class=class>
                <div class="flex items-center justify-between mb-4">
                    <h2 class="text-lg font-semibold text-gray-800 dark:text-gray-100">{title}</h2>
                    <button type="button" class=CLASS_ICON_BTN on:click=close.clone()>
                        <i class="fas fa-times"></i>
                    </button>
                </div>
                {children()}
            </div>
        </div>
    }
}

#[component]
pub fn DeleteConfirmModal(
    entity_name: Box<dyn Fn() -> String + Send>,
    item_name: String,
    action: Action<(), Result<(), String>>,
    on_close: impl Fn() + 'static + Clone + Send,
    on_success: impl Fn() + 'static + Clone + Send,
) -> impl IntoView {
    let toast = use_toast();

    Effect::new(move |_| {
        if let Some(Ok(_)) = action.value().get() {
            let en = entity_name();
            let act = ts!(ActionDeleted);
            toast.success(trs!(EntityAction, &[("entity", &en), ("action", &act)]));
            on_success();
        }
    });

    let on_delete = move |_: leptos::ev::MouseEvent| {
        if action.pending().get_untracked() {
            return;
        }
        action.dispatch(());
    };

    view! {
        <ModalShell on_close=on_close.clone() title=ts!(DeleteConfirmTitle)>
            <p class="text-gray-600 dark:text-gray-400 text-sm mb-6">
                {tr!(DeleteConfirmMessage, &[("name", &item_name)])}
            </p>
            <div class="flex items-center justify-end gap-3">
                <button type="button" class=CLASS_BTN_CANCEL on:click=move |_| on_close()>
                    {t!(Cancel)}
                </button>
                <button
                    type="button"
                    disabled=move || action.pending().get()
                    class=CLASS_BTN_DANGER
                    on:click=on_delete
                >
                    {move || {
                        if action.pending().get_untracked() {
                            view! {
                                <>
                                    <i class="fas fa-spinner fa-spin"></i>
                                    {t!(Delete)}
                                </>
                            }
                                .into_any()
                        } else {
                            view! { {t!(Delete)} }.into_any()
                        }
                    }}
                </button>
            </div>
        </ModalShell>
    }
}
