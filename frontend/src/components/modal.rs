use leptos::ev;
use leptos::prelude::*;

use crate::components::error_display::ErrorText;
use crate::components::style::{
    CLASS_BTN_CANCEL, CLASS_BTN_DANGER, CLASS_FORM_FOOTER, CLASS_ICON_BTN,
};
use crate::components::table::SubmitButton;
use crate::components::toast::use_toast;
use crate::{t, tr, trs, ts};

/// One-shot modal shell. All props are static (not reactive): the overlay
/// covers the whole viewport, so no other UI can be operated while the modal
/// is open, and every prop is re-computed whenever the modal is re-created.
#[component]
pub fn ModalShell(
    on_close: impl Fn() + 'static + Clone + Send,
    title: String,
    #[prop(default = "max-w-md")] card_class: &'static str,
    children: Children,
) -> impl IntoView {
    let class = format!(
        "relative z-10 bg-white dark:bg-ink-900 rounded-xl p-6 shadow-2xl w-full mx-4 {}",
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
                    <h2 class="text-lg font-semibold text-gray-800 dark:text-ink-100">{title}</h2>
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
    let consumed = RwSignal::new(false);

    Effect::new(move |_| {
        if action.pending().get() {
            consumed.set(false);
            return;
        }
        if consumed.get_untracked() {
            return;
        }
        match action.value().get() {
            Some(Ok(_)) => {
                consumed.set(true);
                let en = entity_name();
                let act = ts!(ActionDeleted);
                toast.success(trs!(EntityAction, &[("entity", &en), ("action", &act)]));
                on_success();
            }
            Some(Err(e)) => {
                consumed.set(true);
                toast.error(e);
            }
            None => {}
        }
    });

    let on_delete = move |_: leptos::ev::MouseEvent| {
        if action.pending().get_untracked() {
            return;
        }
        action.dispatch(());
    };

    // Closing mid-flight disposes the Effect above, dropping the delete result
    // (and its toast) even though the delete already happened upstream.
    let close_when_idle = {
        let pending = action.pending();
        move || {
            if !pending.get_untracked() {
                on_close();
            }
        }
    };
    let close_cancel = close_when_idle.clone();

    view! {
        <ModalShell on_close=close_when_idle title=ts!(DeleteConfirmTitle)>
            <p class="text-gray-600 dark:text-ink-400 text-sm mb-6">
                {tr!(DeleteConfirmMessage, &[("name", &item_name)])}
            </p>
            <div class="flex items-center justify-end gap-3">
                <button
                    type="button"
                    class=CLASS_BTN_CANCEL
                    disabled=move || action.pending().get()
                    on:click=move |_| close_cancel()
                >
                    {t!(Cancel)}
                </button>
                <button
                    type="button"
                    disabled=move || action.pending().get()
                    class=CLASS_BTN_DANGER
                    on:click=on_delete
                >
                    {move || {
                        if action.pending().get() {
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

#[component]
pub fn FormModalShell(
    on_close: impl Fn() + 'static + Clone + Send,
    title: String,
    on_submit: impl Fn(leptos::ev::SubmitEvent) + 'static + Clone + Send,
    #[prop(into)] pending: Signal<bool>,
    is_edit: bool,
    form_error: RwSignal<String>,
    children: Children,
) -> impl IntoView {
    // Closing while the save is in flight disposes the caller's save Effect,
    // so a successful write would land on the backend with no store update and
    // no toast. Requests are capped by the 30s timeout in api.rs, so the modal
    // can never be stuck open indefinitely.
    let close_when_idle = move || {
        if !pending.get_untracked() {
            on_close();
        }
    };
    let close_cancel = close_when_idle.clone();

    view! {
        <ModalShell on_close=close_when_idle title=title>
            <form on:submit=on_submit class="space-y-4">
                {children()}
                <ErrorText msg=form_error />
                <div class=CLASS_FORM_FOOTER>
                    <button
                        type="button"
                        class=CLASS_BTN_CANCEL
                        disabled=move || pending.get()
                        on:click=move |_| close_cancel()
                    >
                        {t!(Cancel)}
                    </button>
                    <SubmitButton is_edit=is_edit pending=pending />
                </div>
            </form>
        </ModalShell>
    }
}
