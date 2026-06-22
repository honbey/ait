use sycamore::prelude::*;
use sycamore::web::tags::*;

use super::modal::{form_delete_footer, modal_dialog, modal_title};
use crate::i18n::{I18n, K};

pub fn render_delete_confirm(
    message: String,
    deleting: Signal<bool>,
    on_confirm: impl Fn(web_sys::MouseEvent) + 'static,
    on_cancel: impl Fn(web_sys::MouseEvent) + Clone + 'static,
) -> View {
    let i18n = use_context::<I18n>();
    let on_cancel_title = on_cancel.clone();
    let on_cancel_footer = on_cancel.clone();
    let on_cancel_backdrop = on_cancel;
    modal_dialog(
        (
            modal_title(i18n.t(K::DeleteConfirmTitle), on_cancel_title),
            p().class("text-gray-600 dark:text-gray-400 text-sm mb-6")
                .children(message),
            form_delete_footer(
                i18n.t(K::Cancel),
                on_cancel_footer,
                deleting,
                i18n.t(K::Delete),
                on_confirm,
            ),
        ),
        on_cancel_backdrop,
    )
}
