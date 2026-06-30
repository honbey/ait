use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::components::modal::{CLASS_INPUT, CLASS_LABEL};

pub fn auth_page_shell(
    on_submit: impl Fn(web_sys::SubmitEvent) + 'static,
    title: String,
    children: impl Into<View>,
) -> View {
    form()
        .on(events::submit, on_submit)
        .class("min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900")
        .children(
            div()
                .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm p-8 w-full max-w-md mx-4")
                .children((
                    h2()
                        .class("text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6 text-center")
                        .children(title),
                    children,
                )),
        )
        .into()
}

pub fn auth_input_field(
    label_text: String,
    input_type: &'static str,
    placeholder: String,
    value: Signal<String>,
    error: Signal<String>,
    is_last: bool,
) -> View {
    let margin = if is_last { "mb-6" } else { "mb-4" };
    div()
        .class(margin)
        .children((
            label().class(CLASS_LABEL).children(label_text),
            input()
                .class(CLASS_INPUT)
                .attr("type", input_type)
                .attr("placeholder", placeholder)
                .bind(bind::value, value)
                .on(events::input, move |_| error.set(String::new())),
        ))
        .into()
}

pub fn auth_error_display(error: Signal<String>) -> View {
    View::from_dynamic(move || {
        let err = error.get_clone();
        if err.is_empty() {
            View::new()
        } else {
            p().class("text-red-500 text-sm mb-4").children(err).into()
        }
    })
}

pub fn auth_submit_button(loading: Signal<bool>, button_text: String) -> View {
    button()
        .attr("type", "submit")
        .disabled(move || loading.get())
        .class("w-full py-2 px-4 bg-indigo-600 hover:enabled:bg-indigo-700 text-white font-semibold rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2")
        .children(View::from_dynamic(move || -> View {
            if loading.get() {
                div().class("flex items-center gap-2").children((
                    i().class("fas fa-spinner animate-spin"),
                    span().children(button_text.clone()),
                )).into()
            } else {
                span().children(button_text.clone()).into()
            }
        }))
        .into()
}

pub fn auth_link_footer(
    text: String,
    link_text: String,
    on_click: impl Fn(web_sys::MouseEvent) + 'static,
) -> View {
    div().class("mt-4 text-center").children((
        span().class("text-sm text-gray-500 dark:text-gray-400")
            .children(text),
        button()
            .class("ml-1 text-sm text-indigo-600 dark:text-indigo-400 hover:underline cursor-pointer")
            .on(events::click, on_click)
            .children(link_text),
    )).into()
}
