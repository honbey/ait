use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::modal::CLASS_PAGE_SHELL;

fn sk(classes: &str) -> View {
    div()
        .class(format!("skeleton bg-gray-200 dark:bg-gray-700 {}", classes))
        .into()
}

pub fn dashboard_skeleton() -> View {
    div()
        .class(format!("{} space-y-6 sm:space-y-8", CLASS_PAGE_SHELL))
        .children((
            div()
                .class("grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6")
                .children((
                    stat_card_skeleton(),
                    stat_card_skeleton(),
                    stat_card_skeleton(),
                    stat_card_skeleton(),
                )),
            div()
                .class("grid grid-cols-1 lg:grid-cols-2 gap-6")
                .children((chart_area_skeleton(), chart_area_skeleton())),
        ))
        .into()
}

pub fn table_skeleton() -> View {
    let toolbar = div()
        .class("flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700")
        .children((
            sk("h-6 w-24 rounded"),
            sk("h-9 w-20 rounded-lg"),
        ));

    let rows: Vec<View> = (0..6)
        .map(|i| {
            let cls = if i == 5 {
                "border-b-0"
            } else {
                "border-b border-gray-200 dark:border-gray-700"
            };
            div()
                .class(format!("flex items-center gap-4 px-6 py-4 {}", cls))
                .children((
                    sk("h-5 w-28 rounded"),
                    sk("h-5 w-24 rounded"),
                    sk("h-5 w-20 rounded"),
                    sk("h-5 w-32 rounded"),
                ))
                .into()
        })
        .collect();

    div()
        .class(CLASS_PAGE_SHELL)
        .children(
            div()
                .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 overflow-hidden")
                .children({
                    let mut all: Vec<View> = Vec::new();
                    all.push(toolbar.into());
                    all.extend(rows);
                    all
                }),
        )
        .into()
}

pub fn text_gen_skeleton() -> View {
    div()
        .class(CLASS_PAGE_SHELL)
        .children(
            div()
                .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 max-w-6xl mx-auto")
                .children(sk("w-full h-96 rounded-lg")),
        )
        .into()
}

fn stat_card_skeleton() -> View {
    div()
        .class("bg-white dark:bg-gray-800 rounded-xl p-6 flex items-center gap-4 shadow-sm")
        .children((
            sk("w-14 h-14 rounded-full shrink-0"),
            div()
                .class("flex flex-col gap-3")
                .children((sk("w-24 h-8 rounded"), sk("w-16 h-4 rounded"))),
        ))
        .into()
}

fn chart_area_skeleton() -> View {
    div()
        .class("bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm")
        .children((sk("w-40 h-5 mb-4 rounded"), sk("w-full h-64 rounded-lg")))
        .into()
}
