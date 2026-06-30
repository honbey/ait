use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::components::modal::{CLASS_CARD, zebra_bg};
use crate::i18n::{I18n, K};

pub struct Column<T: 'static> {
    pub header: View,
    pub cell: Rc<dyn Fn(T) -> View>,
}

pub enum CrudModal<T> {
    Closed,
    Detail(T),
    Add,
    Edit(T),
    Delete(T),
}

impl<T> Clone for CrudModal<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        match self {
            Self::Closed => Self::Closed,
            Self::Detail(a) => Self::Detail(a.clone()),
            Self::Add => Self::Add,
            Self::Edit(a) => Self::Edit(a.clone()),
            Self::Delete(a) => Self::Delete(a.clone()),
        }
    }
}

pub fn common_table<T: Clone + 'static>(
    title: String,
    items: Vec<T>,
    refreshing: Signal<bool>,
    on_refresh: impl Fn() + 'static,
    columns: Vec<Column<T>>,
    add_button: View,
    modals: Vec<View>,
) -> View {
    let i18n = use_context::<I18n>();
    let count = items.len();

    let cells: Vec<Rc<dyn Fn(T) -> View>> = columns.iter().map(|col| col.cell.clone()).collect();
    let headers: Vec<View> = columns.into_iter().map(|col| col.header).collect();

    let rows: Vec<View> = items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            let cols: Vec<View> = cells.iter().map(|cell| (cell)(item.clone())).collect();
            tr().class(zebra_bg(idx)).children(cols).into()
        })
        .collect();

    let header = render_table_header(i18n, title, count, refreshing, on_refresh, add_button);
    let table = table_shell(headers, rows);
    table_container(header, table, modals)
}

pub fn debounce_refresh(refresh: Signal<usize>, refreshing: Signal<bool>) -> impl Fn() + 'static {
    let pending: Rc<RefCell<Option<Timeout>>> = Rc::new(RefCell::new(None));
    move || {
        refreshing.set(true);
        let timer = Timeout::new(50, move || {
            refresh.update(|v| *v += 1);
        });
        *pending.borrow_mut() = Some(timer);
    }
}

pub fn render_table_header(
    i18n: I18n,
    title: String,
    count: usize,
    refreshing: Signal<bool>,
    on_refresh: impl Fn() + 'static,
    add_button: View,
) -> View {
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
        .class(format!("{} overflow-hidden", CLASS_CARD))
        .children((header, table, modals))
        .into()
}

// --- Modal gating helpers (deprecated — use CrudModal enum + single from_dynamic instead) ---
