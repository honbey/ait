use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;

pub fn modal_dialog(
    children: impl Into<View>,
    on_close: impl Fn(web_sys::MouseEvent) + 'static,
) -> View {
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

pub fn form_field(id: String, label_text: String, input: View) -> View {
    div()
        .children((
            label()
                .attr("for", id)
                .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                .children(label_text),
            input,
        ))
        .into()
}

pub fn form_input(id: String, placeholder: String, value: Signal<String>) -> View {
    input()
        .attr("id", id)
        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
        .attr("type", "text")
        .attr("placeholder", placeholder)
        .bind(bind::value, value)
        .into()
}

pub fn form_checkbox(id: String, label_text: String, checked: Signal<bool>) -> View {
    div()
        .class("flex items-center gap-2")
        .children((
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

// --- Table helpers ---

pub fn status_badge(enabled: bool, enabled_text: &str, disabled_text: &str) -> View {
    let (bg, text) = if enabled {
        (
            "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400",
            enabled_text,
        )
    } else {
        (
            "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400",
            disabled_text,
        )
    };
    span()
        .class(format!(
            "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
            bg
        ))
        .children(text.to_string())
        .into()
}

pub fn zebra_bg(idx: usize) -> &'static str {
    if idx.is_multiple_of(2) {
        ""
    } else {
        "bg-gray-50 dark:bg-gray-800/50"
    }
}

pub fn icon_button(icon: &str, on_click: impl Fn(web_sys::MouseEvent) + 'static) -> View {
    button()
        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
        .on(events::click, on_click)
        .children(i().class(format!("{} text-xs", icon)))
        .into()
}

pub fn action_cell(buttons: impl Into<View>) -> View {
    td().class("px-6 py-4 text-center whitespace-nowrap")
        .children(buttons)
        .into()
}

pub fn text_cell(content: impl Into<View>) -> View {
    td().class("px-6 py-4").children(content).into()
}

pub fn name_cell(name: String) -> View {
    td().class("px-6 py-4 font-medium text-gray-800 dark:text-gray-200")
        .children(name)
        .into()
}

pub fn secondary_cell(content: String) -> View {
    td().class("px-6 py-4 text-gray-600 dark:text-gray-400")
        .children(content)
        .into()
}

pub fn mono_cell(content: String) -> View {
    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono")
        .children(content)
        .into()
}

pub fn timestamp_cell(ts: f64) -> View {
    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
        .children(crate::models::format_timestamp(ts))
        .into()
}

pub fn render_detail_modal(
    title: String,
    rows: Vec<(String, String)>,
    on_close: impl Fn(web_sys::MouseEvent) + Clone + 'static,
) -> View {
    let on_close_clone = on_close.clone();
    let row_views: Vec<View> = rows
        .into_iter()
        .map(|(label, value)| detail_row(label, value))
        .collect();
    modal_dialog((modal_title(title, on_close_clone), row_views), on_close)
}

pub fn select_input(id: String, value: Signal<String>, options: Vec<(String, String)>) -> View {
    let option_views: Vec<View> = options
        .into_iter()
        .map(|(val, label)| {
            option()
                .attr("value", val.clone())
                .bool_attr("selected", {
                    let v = value;
                    let val = val.clone();
                    move || v.get_clone() == val
                })
                .children(label)
                .into()
        })
        .collect();
    select()
        .attr("id", id)
        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
        .bind(bind::value, value)
        .children(option_views)
        .into()
}
