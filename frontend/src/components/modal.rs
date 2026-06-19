use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;

pub fn modal_dialog(children: impl Into<View>, on_close: impl Fn(web_sys::MouseEvent) + 'static) -> View {
    let content: View = children.into();
    div()
        .class("fixed inset-0 z-50 flex items-center justify-center")
        .children((
            div()
                .class("absolute inset-0 bg-black/50")
                .on(events::click, on_close),
            div()
                .class("relative z-10 bg-white dark:bg-gray-800 rounded-xl p-6 shadow-2xl max-w-md w-full mx-4")
                .children(content),
        ))
        .into()
}

pub fn modal_title(title: String, on_close: impl Fn(web_sys::MouseEvent) + 'static) -> View {
    div()
        .class("flex items-center justify-between mb-4")
        .children((
            h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100")
                .children(title),
            button()
                .attr("type", "button")
                .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                .on(events::click, on_close)
                .children(i().class("fas fa-times")),
        ))
        .into()
}

pub fn detail_row(label: String, value: String) -> View {
    div()
        .class("flex justify-between items-center py-2 border-b border-gray-100 dark:border-gray-700 last:border-0")
        .children((
            span().class("text-gray-500 dark:text-gray-400 text-sm").children(label),
            span().class("text-gray-900 dark:text-gray-100 font-medium text-sm text-right ml-4 truncate").children(value),
        ))
        .into()
}

pub fn form_field(label_text: String, input: View) -> View {
    div().children((
        label()
            .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
            .children(label_text),
        input,
    ))
    .into()
}

pub fn form_input(placeholder: String, value: Signal<String>) -> View {
    input()
        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
        .attr("type", "text")
        .attr("placeholder", placeholder)
        .bind(bind::value, value)
        .into()
}

pub fn form_checkbox(id: String, label_text: String, checked: Signal<bool>) -> View {
    div().class("flex items-center gap-2").children((
        input()
            .attr("type", "checkbox")
            .attr("id", id.clone())
            .bind(bind::checked, checked),
        label()
            .attr("for", id)
            .class("text-sm text-gray-700 dark:text-gray-300")
            .children(label_text),
    ))
    .into()
}

pub fn form_error(error: Signal<String>) -> View {
    View::from_dynamic(move || {
        let msg = error.get_clone();
        if msg.is_empty() {
            View::new()
        } else {
            p().class("text-red-500 text-sm").children(msg).into()
        }
    })
}

pub fn form_submit_footer(
    cancel_text: String,
    on_cancel: impl Fn(web_sys::MouseEvent) + 'static,
    loading: Signal<bool>,
    submit_text: String,
) -> View {
    let st = submit_text;
    div().class("flex items-center justify-end gap-3").children((
        button()
            .attr("type", "button")
            .class("px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 cursor-pointer transition-colors")
            .on(events::click, on_cancel)
            .children(cancel_text),
        button()
            .attr("type", "submit")
            .disabled(move || loading.get())
            .class("px-4 py-2 bg-blue-500 hover:enabled:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2")
            .children(View::from_dynamic(move || -> View {
                if loading.get() {
                    div().class("flex items-center gap-2").children((
                        i().class("fas fa-spinner animate-spin"),
                        span().children(st.clone()),
                    )).into()
                } else {
                    span().children(st.clone()).into()
                }
            })),
    ))
    .into()
}

pub fn form_delete_footer(
    cancel_text: String,
    on_cancel: impl Fn(web_sys::MouseEvent) + 'static,
    deleting: Signal<bool>,
    delete_text: String,
    on_delete: impl Fn(web_sys::MouseEvent) + 'static,
) -> View {
    let dt = delete_text;
    div().class("flex items-center justify-end gap-3").children((
        button()
            .attr("type", "button")
            .class("px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 cursor-pointer transition-colors")
            .on(events::click, on_cancel)
            .children(cancel_text),
        button()
            .attr("type", "button")
            .disabled(move || deleting.get())
            .class("px-4 py-2 bg-red-500 hover:enabled:bg-red-600 text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2")
            .on(events::click, on_delete)
            .children(View::from_dynamic(move || -> View {
                if deleting.get() {
                    div().class("flex items-center gap-2").children((
                        i().class("fas fa-spinner animate-spin"),
                        span().children(dt.clone()),
                    )).into()
                } else {
                    span().children(dt.clone()).into()
                }
            })),
    ))
    .into()
}
