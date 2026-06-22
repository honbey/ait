use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::i18n::{I18n, K};

pub fn debounce_refresh(refresh: Signal<usize>, refreshing: Signal<bool>) -> impl Fn() + 'static {
    move || {
        refreshing.set(true);
        let r = refresh;
        Timeout::new(50, move || {
            r.update(|v| *v += 1);
        })
        .forget();
    }
}

pub fn render_table_header(
    title: String,
    count: usize,
    refreshing: Signal<bool>,
    on_refresh: impl Fn() + 'static,
    add_button: View,
) -> View {
    let i18n = use_context::<I18n>();
    div()
        .class("p-6 border-b border-gray-100 dark:border-gray-700 flex items-center justify-between")
        .children((
            div().class("flex items-center gap-3").children((
                h2().class("text-xl font-semibold text-gray-800 dark:text-gray-100")
                    .children(title),
                span().class(
                    "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                )
                .children(i18n.t_replace(K::TotalCount, "count", &count.to_string())),
                button()
                    .disabled(move || refreshing.get())
                    .class("text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed")
                    .on(events::click, move |_| {
                        if refreshing.get() { return; }
                        on_refresh();
                    })
                    .children(i().class(move || {
                        if refreshing.get() { "fas fa-sync-alt animate-spin" } else { "fas fa-sync-alt" }
                    })),
            )),
            add_button,
        ))
        .into()
}

pub fn render_add_button(is_admin: Signal<bool>, make_button: impl Fn() -> View + 'static) -> View {
    View::from_dynamic::<View>(move || {
        if is_admin.get() {
            make_button()
        } else {
            View::new()
        }
    })
}

pub fn table_shell(headers: Vec<View>, rows: Vec<View>) -> View {
    div()
        .class("overflow-x-auto")
        .children(
            table().class("w-full text-sm").children((
                thead().children(
                    tr().class("border-b border-gray-100 dark:border-gray-700")
                        .children(headers),
                ),
                tbody().children(rows),
            )),
        )
        .into()
}

pub fn th_left(label: String) -> View {
    th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
        .children(label)
        .into()
}

pub fn th_center(label: String) -> View {
    th().class("text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
        .children(label)
        .into()
}

pub fn table_container(header: View, table: View, modals: Vec<View>) -> View {
    div()
        .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm overflow-hidden")
        .children((header, table, modals))
        .into()
}
