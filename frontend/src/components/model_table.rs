use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::i18n::I18n;
use crate::models::Model;

fn detail_row(label: String, value: String) -> View {
    div()
        .class("flex justify-between items-center py-2 border-b border-gray-100 dark:border-gray-700 last:border-0")
        .children((
            span().class("text-gray-500 dark:text-gray-400 text-sm").children(label),
            span().class("text-gray-900 dark:text-gray-100 font-medium text-sm text-right ml-4 truncate").children(value),
        ))
        .into()
}

fn render_detail_modal(i18n: &I18n, model: Model, show_detail: Signal<Option<usize>>) -> View {
    let enabled_text = i18n.t("providers_status_enabled");
    let disabled_text = i18n.t("providers_status_disabled");
    let status = if model.enabled { enabled_text } else { disabled_text };

    div()
        .class("fixed inset-0 z-50 flex items-center justify-center")
        .children((
            div()
                .class("absolute inset-0 bg-black/50")
                .on(events::click, move |_| show_detail.set(None)),
            div()
                .class("relative z-10 bg-white dark:bg-gray-800 rounded-xl p-6 shadow-2xl max-w-md w-full mx-4")
                .children((
                    div()
                        .class("flex items-center justify-between mb-4")
                        .children((
                            h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100")
                                .children(i18n.t("model_detail")),
                            button()
                                .class("text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                .on(events::click, move |_| show_detail.set(None))
                                .children(i().class("fas fa-times")),
                        )),
                    detail_row("ID".to_string(), model.id),
                    detail_row(i18n.t("models_table_name"), model.name),
                    detail_row(i18n.t("models_table_provider_id"), model.provider_id),
                    detail_row(i18n.t("models_table_upstream_model"), model.upstream_model),
                    detail_row(i18n.t("providers_table_status"), status),
                    detail_row(i18n.t("created_at"), format!("{}", model.created_at as u64)),
                )),
        ))
        .into()
}

fn make_model_rows(
    models: Vec<Model>,
    i18n: &I18n,
    show_detail: Signal<Option<usize>>,
) -> Vec<View> {
    let enabled_text = i18n.t("providers_status_enabled");
    let disabled_text = i18n.t("providers_status_disabled");
    models
        .into_iter()
        .enumerate()
        .map(|(idx, m)| {
            let ec = if m.enabled {
                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
            } else {
                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
            };
            let st = if m.enabled { &enabled_text } else { &disabled_text };
            let bg = if idx % 2 == 0 { "" } else { "bg-gray-50 dark:bg-gray-800/50" };
            let span_class = format!(
                "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
                ec
            );
            let show = show_detail;
            tr().class(bg)
                .children((
                    td().class("px-6 py-4 font-medium text-gray-800 dark:text-gray-200")
                        .children(m.name),
                    td().class("px-6 py-4 text-gray-600 dark:text-gray-400")
                        .children(m.provider_id),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono")
                        .children(m.upstream_model),
                    td().class("px-6 py-4")
                        .children(span().class(span_class).children(st.clone())),
                    td().class("px-6 py-4 text-center")
                        .children(
                            button()
                                .class("cursor-pointer text-gray-400")
                                .on(events::click, move |_| show.set(Some(idx)))
                                .children(i().class("fas fa-ellipsis-h")),
                        ),
                ))
                .into()
        })
        .collect()
}

#[derive(Props)]
pub struct ModelTableProps {
    pub models: Vec<Model>,
}

#[component]
pub fn ModelTable(props: ModelTableProps) -> View {
    let i18n = use_context::<I18n>();
    let show_detail = create_signal::<Option<usize>>(None);
    let models = props.models;
    let rows = make_model_rows(models.clone(), &i18n, show_detail);

    let i18n_modal = i18n.clone();
    let modal = View::from_dynamic(move || match show_detail.get() {
        Some(idx) => models.get(idx).map_or(View::new(), |m| {
            render_detail_modal(&i18n_modal, m.clone(), show_detail)
        }),
        None => View::new(),
    });

    div()
        .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm overflow-hidden mt-8")
        .children((
            div()
                .class("p-6 border-b border-gray-100 dark:border-gray-700")
                .children(
                    h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100")
                        .children(i18n.t("models_table_title")),
                ),
            div().class("overflow-x-auto").children(
                table().class("w-full text-sm").children((
                    thead().children(
                        tr().class("border-b border-gray-100 dark:border-gray-700")
                            .children((
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("models_table_name")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("models_table_provider_id")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("models_table_upstream_model")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("models_table_status")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            ),
                        )),
                    ),
                    tbody().children(rows),
                )),
            ),
            modal,
        ))
        .into()
}
