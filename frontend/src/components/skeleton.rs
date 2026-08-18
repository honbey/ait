use leptos::prelude::*;

use crate::components::style::{CLASS_BORDER_B, CLASS_CARD};

fn sk(extra: &str) -> impl IntoView {
    view! { <div class=format!("skeleton bg-gray-200 dark:bg-gray-600 {}", extra)></div> }
}

fn stat_card() -> impl IntoView {
    view! {
        <div class="bg-white dark:bg-gray-800 rounded-xl p-6 flex items-center gap-4 shadow-sm">
            <div class="w-14 h-14 rounded-full skeleton bg-gray-200 dark:bg-gray-600 shrink-0"></div>
            <div class="flex-1 space-y-3">{sk("h-8 w-24 rounded")}{sk("h-4 w-16 rounded")}</div>
        </div>
    }
}

fn chart_card() -> impl IntoView {
    view! {
        <div class=format!(
            "{} p-6",
            CLASS_CARD,
        )>{sk("h-5 w-48 rounded mb-4")} {sk("h-48 rounded-lg w-full")}</div>
    }
}

pub fn overview_skeleton() -> impl IntoView {
    view! {
        <div class="space-y-6 sm:space-y-8">
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-6">
                {stat_card()}{stat_card()}{stat_card()}
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-6">
                {stat_card()}{stat_card()}{stat_card()}
            </div>
            {chart_card()}
        </div>
    }
}

fn table_rows() -> Vec<impl IntoView> {
    (0..5)
        .map(|_| {
            view! {
                <tr class=format!("{} last:border-b-0", CLASS_BORDER_B)>
                    <td class="px-6 py-4">{sk("h-4 w-28 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-24 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-36 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-5 w-16 rounded-full")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-28 rounded")}</td>
                    <td class="px-6 py-4 text-center">{sk("h-4 w-12 rounded mx-auto")}</td>
                </tr>
            }
        })
        .collect()
}

pub fn table_skeleton() -> impl IntoView {
    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class=CLASS_BORDER_B>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-16 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-14 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-14 rounded")}</th>
                        <th class="px-6 py-3 text-center">{sk("h-4 w-10 rounded mx-auto")}</th>
                    </tr>
                </thead>
                <tbody>{table_rows()}</tbody>
            </table>
        </div>
    }
}

fn logs_table_rows() -> Vec<impl IntoView> {
    (0..5)
        .map(|_| {
            view! {
                <tr class=format!("{} last:border-b-0", CLASS_BORDER_B)>
                    <td class="px-6 py-4">{sk("h-4 w-32 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-24 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-28 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-16 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-16 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-16 rounded")}</td>
                    <td class="px-6 py-4">{sk("h-4 w-28 rounded")}</td>
                    <td class="px-6 py-4 text-center">{sk("h-5 w-16 rounded-full mx-auto")}</td>
                </tr>
            }
        })
        .collect()
}

pub fn logs_table_skeleton() -> impl IntoView {
    view! {
        <div class="overflow-x-auto">
            <table class="w-full text-sm">
                <thead>
                    <tr class=CLASS_BORDER_B>
                        <th class="px-6 py-3">{sk("h-4 w-14 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-10 rounded")}</th>
                        <th class="px-6 py-3">{sk("h-4 w-12 rounded")}</th>
                        <th class="px-6 py-3 text-center">{sk("h-4 w-10 rounded mx-auto")}</th>
                    </tr>
                </thead>
                <tbody>{logs_table_rows()}</tbody>
            </table>
        </div>
    }
}
