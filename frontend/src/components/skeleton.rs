use leptos::prelude::*;

use crate::components::style::{CLASS_BORDER_B, CLASS_CARD};

fn sk(extra: &str) -> impl IntoView {
    view! { <div class=format!("skeleton bg-gray-200 dark:bg-gray-600 {}", extra)></div> }
}

fn stat_card() -> impl IntoView {
    view! {
        <div class="bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm">
            <div class="flex items-center gap-4">
                <div class="w-14 h-14 rounded-full skeleton bg-gray-200 dark:bg-gray-600 shrink-0"></div>
                <div class="flex-1 space-y-3">
                    {sk("h-8 w-24 rounded")} {sk("h-4 w-16 rounded")}
                </div>
            </div>
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
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6">
                {stat_card()} {stat_card()} {stat_card()} {stat_card()}
            </div>
            <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">{chart_card()} {chart_card()}</div>
        </div>
    }
}

pub fn render_loading() -> impl IntoView {
    view! {
        <div class="min-h-[calc(100vh-3.5rem)] bg-gray-50 dark:bg-gray-900 flex items-center justify-center">
            <div class="w-32 h-32 rounded-full bg-indigo-200 dark:bg-indigo-700 flex items-center justify-center">
                <svg
                    class="animate-spin text-indigo-600 dark:text-indigo-400"
                    xmlns="http://www.w3.org/2000/svg"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <circle
                        class="opacity-25"
                        cx="12"
                        cy="12"
                        r="10"
                        stroke="currentColor"
                        stroke-width="4"
                    ></circle>
                    <path
                        class="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l 3-2.647z"
                    ></path>
                </svg>
            </div>
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
        <div class=CLASS_CARD>
            <div class=format!("p-6 {} flex items-center justify-between", CLASS_BORDER_B)>
                <div class="flex items-center gap-3">
                    {sk("h-6 w-15 rounded-full")} {sk("h-5 w-5 rounded")}
                </div>
                {sk("h-9 w-28 rounded-lg")}
            </div>
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
        </div>
    }
}
