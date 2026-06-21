use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_model, delete_model, update_model};
use crate::components::modal::{
    detail_row, form_checkbox, form_delete_footer, form_error, form_field, form_input,
    form_submit_footer, modal_dialog, modal_title,
};
use crate::i18n::I18n;
use crate::models::{Model, Provider, format_timestamp};

fn render_detail_modal(i18n: &I18n, model: Model, show_detail: Signal<Option<usize>>) -> View {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    let status = if model.enabled {
        enabled_text
    } else {
        disabled_text
    };

    modal_dialog(
        (
            modal_title(
                i18n.t_replace("detail_title", "entity", &i18n.t("models")),
                move |_| show_detail.set(None),
            ),
            detail_row("ID".to_string(), model.id),
            detail_row(i18n.t("name"), model.name),
            detail_row(i18n.t("model_provider_id"), model.provider_id),
            detail_row(i18n.t("model_upstream_model"), model.upstream_model),
            detail_row(i18n.t("table_status"), status),
            detail_row(i18n.t("created_at"), format_timestamp(model.created_at)),
            detail_row(i18n.t("updated_at"), format_timestamp(model.updated_at)),
        ),
        move |_| show_detail.set(None),
    )
}

fn render_add_modal(
    i18n: &I18n,
    providers: Vec<Provider>,
    model_refresh: Signal<usize>,
    show_add_modal: Signal<bool>,
) -> View {
    let form_name = create_signal(String::new());
    let form_provider_id = create_signal(String::new());
    let form_upstream = create_signal(String::new());
    let form_enabled = create_signal(true);
    let form_err = create_signal(String::new());
    let form_loading = create_signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let p = form_provider_id.get_clone();
        if n.is_empty() || p.is_empty() {
            form_err.set("Name and Provider ID are required".to_string());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let name = n;
        let provider_id = p;
        let upstream = form_upstream.get_clone();
        let enabled = form_enabled.get();
        let refresh = model_refresh;
        let loading = form_loading;
        spawn_local_scoped(async move {
            match create_model(&name, &provider_id, &upstream, enabled).await {
                Ok(_) => {
                    refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    loading.set(false);
                    form_err.set(e.to_string());
                }
            }
        });
    };

    let provider_options: Vec<View> = providers
        .into_iter()
        .map(|p| {
            option()
                .attr("value", p.id.clone())
                .children(format!("{} ({})", p.name, p.provider_type))
                .into()
        })
        .collect();

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t("model_add"), move |_| show_add_modal.set(false)),
                form_field(i18n.t("name"), form_input(i18n.t("name"), form_name)),
                form_field(i18n.t("model_provider_id"),
                    select()
                        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                        .bind(sycamore::web::bind::value, form_provider_id)
                        .children(provider_options).into()),
                form_field(i18n.t("model_upstream_model"), form_input(i18n.t("model_upstream_model"), form_upstream)),
                form_checkbox("model-enabled".to_string(), i18n.t("status_enabled"), form_enabled),
                form_error(form_err),
                form_submit_footer(
                    i18n.t("cancel"),
                    move |_| show_add_modal.set(false),
                    form_loading,
                    i18n.t("save"),
                ),
            )),
        move |_| show_add_modal.set(false),
    )
}

fn render_edit_modal(
    i18n: &I18n,
    providers: Vec<Provider>,
    model_refresh: Signal<usize>,
    show_edit_modal: Signal<Option<Model>>,
    model: Model,
) -> View {
    let form_name = create_signal(model.name.clone());
    let form_provider_id = create_signal(model.provider_id.clone());
    let form_upstream = create_signal(model.upstream_model.clone());
    let form_enabled = create_signal(model.enabled);
    let form_err = create_signal(String::new());
    let form_loading = create_signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let p = form_provider_id.get_clone();
        if n.is_empty() || p.is_empty() {
            form_err.set("Name and Provider ID are required".to_string());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let model_name = model.name.clone();
        let provider_id = p;
        let upstream = form_upstream.get_clone();
        let enabled = form_enabled.get();
        let refresh = model_refresh;
        let loading = form_loading;
        let err = form_err;
        spawn_local_scoped(async move {
            match update_model(&model_name, &provider_id, &upstream, enabled).await {
                Ok(_) => {
                    refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    loading.set(false);
                    err.set(e.to_string());
                }
            }
        });
    };

    let provider_options: Vec<View> = providers
        .into_iter()
        .map(|p| {
            option()
                .attr("value", p.id.clone())
                .bool_attr("selected", move || {
                    form_provider_id.get_clone() == p.id.clone()
                })
                .children(format!("{} ({})", p.name, p.provider_type))
                .into()
        })
        .collect();

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t("model_edit"), move |_| show_edit_modal.set(None)),
                form_field(i18n.t("name"), form_input(i18n.t("name"), form_name)),
                form_field(i18n.t("model_provider_id"),
                    select()
                        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                        .bind(sycamore::web::bind::value, form_provider_id)
                        .children(provider_options).into()),
                form_field(i18n.t("model_upstream_model"), form_input(i18n.t("model_upstream_model"), form_upstream)),
                form_checkbox("edit-enabled".to_string(), i18n.t("status_enabled"), form_enabled),
                form_error(form_err),
                form_submit_footer(
                    i18n.t("cancel"),
                    move |_| show_edit_modal.set(None),
                    form_loading,
                    i18n.t("save"),
                ),
            )),
        move |_| show_edit_modal.set(None),
    )
}

fn render_delete_confirm(
    i18n: &I18n,
    model_refresh: Signal<usize>,
    show_delete_confirm: Signal<Option<Model>>,
    model: Model,
) -> View {
    let deleting = create_signal(false);
    let model_name = model.name.clone();
    let on_delete = move |_| {
        if deleting.get() {
            return;
        }
        deleting.set(true);
        let mn = model_name.clone();
        let refresh = model_refresh;
        let d = deleting;
        spawn_local_scoped(async move {
            match delete_model(&mn).await {
                Ok(_) => {
                    refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    d.set(false);
                    sycamore::web::console_log!("Failed to delete model: {}", e);
                }
            }
        });
    };

    modal_dialog(
        (
            modal_title(i18n.t("delete_confirm_title"), move |_| {
                show_delete_confirm.set(None)
            }),
            p().class("text-gray-600 dark:text-gray-400 text-sm mb-6")
                .children(i18n.t_replace("delete_confirm_message", "name", &model.name)),
            form_delete_footer(
                i18n.t("cancel"),
                move |_| show_delete_confirm.set(None),
                deleting,
                i18n.t("delete"),
                on_delete,
            ),
        ),
        move |_| show_delete_confirm.set(None),
    )
}

fn make_model_rows(
    models: Vec<Model>,
    i18n: &I18n,
    show_detail: Signal<Option<usize>>,
    is_admin: Signal<bool>,
    show_delete_confirm: Signal<Option<Model>>,
    show_edit_modal: Signal<Option<Model>>,
) -> Vec<View> {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    models
        .into_iter()
        .enumerate()
        .map(|(idx, m)| {
            let model_modal = m.clone();
            let ec = if m.enabled {
                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
            } else {
                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
            };
            let st = if m.enabled { &enabled_text } else { &disabled_text };
            let bg = if idx % 2 == 0 { "" } else { "bg-gray-50 dark:bg-gray-800/50" };
            let span_class = format!("inline-block px-2 py-1 rounded-full text-xs font-medium {}", ec);
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
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
                        .children(format_timestamp(m.updated_at)),
                    td().class("px-6 py-4 text-center whitespace-nowrap").children(
                        button()
                            .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                            .on(events::click, move |_| show.set(Some(idx)))
                            .children(i().class("fas fa-eye text-xs")),
                    ),
                    td().class("px-6 py-4 text-center whitespace-nowrap").children(
                        View::from_dynamic::<View>(move || {
                            if is_admin.get() {
                                div().class("flex items-center justify-center gap-3").children((
                                    button()
                                        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                        .on(events::click, {
                                            let p = model_modal.clone();
                                            move |_| show_edit_modal.set(Some(p.clone()))
                                        })
                                        .children(i().class("fas fa-pen text-xs")),
                                    button()
                                        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                        .on(events::click, {
                                            let p = model_modal.clone();
                                            move |_| show_delete_confirm.set(Some(p.clone()))
                                        })
                                        .children(i().class("fas fa-trash text-xs")),
                                )).into()
                            } else {
                                i().class("fas fa-ban text-gray-300 dark:text-gray-600 cursor-not-allowed").into()
                            }
                        })
                    ),
                ))
                .into()
        })
        .collect()
}

#[derive(Props)]
pub struct ModelTableProps {
    pub models: Vec<Model>,
    pub providers: Vec<Provider>,
    pub is_admin: Signal<bool>,
    pub model_refresh: Signal<usize>,
    pub model_refreshing: Signal<bool>,
}

#[component]
pub fn ModelTable(props: ModelTableProps) -> View {
    let i18n = use_context::<I18n>();
    let show_detail = create_signal::<Option<usize>>(None);
    let show_delete_confirm = create_signal::<Option<Model>>(None);
    let show_edit_modal = create_signal::<Option<Model>>(None);
    let models = props.models;
    let providers = props.providers;
    let is_admin = props.is_admin;
    let model_refresh = props.model_refresh;
    let model_refreshing = props.model_refreshing;
    let rows = make_model_rows(
        models.clone(),
        &i18n,
        show_detail,
        is_admin,
        show_delete_confirm,
        show_edit_modal,
    );
    let count = models.len();

    let modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_detail.get() {
            Some(idx) => models.get(idx).map_or(View::new(), |m| {
                render_detail_modal(&i18n, m.clone(), show_detail)
            }),
            None => View::new(),
        }
    });

    let show_add_modal = create_signal(false);
    let add_providers = providers.clone();
    let add_modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || {
            if show_add_modal.get() {
                render_add_modal(&i18n, add_providers.clone(), model_refresh, show_add_modal)
            } else {
                View::new()
            }
        }
    });

    let edit_confirm = View::from_dynamic({
        let i18n = i18n.clone();
        let providers = providers.clone();
        move || match show_edit_modal.get_clone() {
            Some(m) => {
                render_edit_modal(&i18n, providers.clone(), model_refresh, show_edit_modal, m)
            }
            None => View::new(),
        }
    });

    let delete_confirm = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_delete_confirm.get_clone() {
            Some(m) => render_delete_confirm(&i18n, model_refresh, show_delete_confirm, m),
            None => View::new(),
        }
    });

    div()
        .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm overflow-hidden")
        .children((
            div()
                .class("p-6 border-b border-gray-100 dark:border-gray-700 flex items-center justify-between")
                .children((
                    div().class("flex items-center gap-3").children((
                        h2().class("text-xl font-semibold text-gray-800 dark:text-gray-100")
                            .children(i18n.t("model_title")),
                        span().class(
                            "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                        )
                        .children(i18n.t_replace("total_count", "count", &count.to_string())),
                        button()
                            .disabled(move || model_refreshing.get())
                            .class("text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed")
                            .on(events::click, move |_| {
                                if model_refreshing.get() { return; }
                                model_refreshing.set(true);
                                let r = model_refresh;
                                Timeout::new(50, move || { r.update(|v| *v += 1); }).forget();
                            })
                            .children(i().class(move || {
                                if model_refreshing.get() { "fas fa-sync-alt animate-spin" } else { "fas fa-sync-alt" }
                            })),
                    )),
                    View::from_dynamic::<View>({
                        let i18n = i18n.clone();
                        move || {
                            if is_admin.get() {
                                button()
                                    .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer")
                                    .on(events::click, move |_| show_add_modal.set(true))
                                    .children((
                                        i().class("fas fa-plus"),
                                        span().children(i18n.t("model_add")),
                                    ))
                                    .into()
                            } else {
                                View::new()
                            }
                        }
                    }),
                )),
            div().class("overflow-x-auto").children(
                table().class("w-full text-sm").children((
                    thead().children(
                        tr().class("border-b border-gray-100 dark:border-gray-700")
                            .children((
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("name")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("model_provider_id")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("model_upstream_model")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("table_status")),
                                th().class("text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("updated_at")),
                                th().class("text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("provider_detail")),
                                th().class("text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("provider_actions")),
                            )),
                    ),
                    tbody().children(rows),
                )),
            ),
            modal,
            add_modal,
            edit_confirm,
            delete_confirm,
        ))
        .into()
}
