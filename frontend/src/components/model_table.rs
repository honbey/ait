use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_model, delete_model, update_model};
use crate::components::data_table::{
    debounce_refresh, render_add_button, render_table_header, table_container, table_shell,
    th_center, th_left,
};
use crate::components::delete_confirm::render_delete_confirm;
use crate::components::modal::{
    action_cell, form_checkbox, form_error, form_field, form_input, form_submit_footer,
    icon_button, modal_dialog, modal_title, mono_cell, name_cell, render_detail_modal,
    secondary_cell, select_input, status_badge, text_cell, timestamp_cell, zebra_bg,
};
use crate::i18n::{I18n, K};
use crate::models::{Model, Provider};

fn provider_name_by_id(provider_id: &str, providers: &[Provider]) -> String {
    providers
        .iter()
        .find(|p| p.id == provider_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| provider_id.to_string())
}

fn render_model_detail(
    model: Model,
    show_detail: Signal<Option<usize>>,
    providers: &[Provider],
) -> View {
    let i18n = use_context::<I18n>();
    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);
    let status = if model.enabled {
        enabled_text
    } else {
        disabled_text
    };

    render_detail_modal(
        i18n.t_replace(K::DetailTitle, "entity", &i18n.t(K::Models)),
        vec![
            ("ID".into(), model.id),
            (i18n.t(K::Name), model.name),
            (
                i18n.t(K::Providers),
                provider_name_by_id(&model.provider_id, providers),
            ),
            (i18n.t(K::ModelUpstreamModel), model.upstream_model),
            (i18n.t(K::TableStatus), status),
            (
                i18n.t(K::CreatedAt),
                crate::models::format_timestamp(model.created_at),
            ),
            (
                i18n.t(K::UpdatedAt),
                crate::models::format_timestamp(model.updated_at),
            ),
        ],
        move |_| show_detail.set(None),
    )
}

fn provider_select_options(providers: &[Provider]) -> Vec<(String, String)> {
    providers
        .iter()
        .map(|p| (p.id.clone(), format!("{} ({})", p.name, p.provider_type)))
        .collect()
}

fn render_add_modal(
    providers: Vec<Provider>,
    model_refresh: Signal<usize>,
    show_add_modal: Signal<bool>,
) -> View {
    let i18n = use_context::<I18n>();
    let form_name = create_signal(String::new());
    let form_provider_id =
        create_signal(providers.first().map(|p| p.id.clone()).unwrap_or_default());
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
            form_err.set("Name and Provider ID are required".into());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let upstream = form_upstream.get_clone();
        let enabled = form_enabled.get();
        spawn_local_scoped(async move {
            match create_model(&n, &p, &upstream, enabled).await {
                Ok(_) => {
                    form_loading.set(false);
                    show_add_modal.set(false);
                    model_refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    form_loading.set(false);
                    form_err.set(e.to_string());
                }
            }
        });
    };

    let options = provider_select_options(&providers);

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t(K::ModelAdd), move |_| show_add_modal.set(false)),
                form_field(
                    "add-model-name".into(),
                    i18n.t(K::Name),
                    form_input("add-model-name".into(), i18n.t(K::Name), form_name),
                ),
                form_field(
                    "add-model-provider".into(),
                    i18n.t(K::Providers),
                    select_input("add-model-provider".into(), form_provider_id, options),
                ),
                form_field(
                    "add-model-upstream".into(),
                    i18n.t(K::ModelUpstreamModel),
                    form_input(
                        "add-model-upstream".into(),
                        i18n.t(K::ModelUpstreamModel),
                        form_upstream,
                    ),
                ),
                form_checkbox(
                    "model-enabled".into(),
                    i18n.t(K::StatusEnabled),
                    form_enabled,
                ),
                form_error(form_err),
                form_submit_footer(
                    i18n.t(K::Cancel),
                    move |_| show_add_modal.set(false),
                    form_loading,
                    i18n.t(K::Save),
                ),
            )),
        move |_| show_add_modal.set(false),
    )
}

fn render_edit_modal(
    providers: Vec<Provider>,
    model_refresh: Signal<usize>,
    show_edit_modal: Signal<Option<Model>>,
    model: Model,
) -> View {
    let i18n = use_context::<I18n>();
    let model_name_for_display = model.name.clone();
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
            form_err.set("Name and Provider ID are required".into());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let model_name = model.name.clone();
        let upstream = form_upstream.get_clone();
        let enabled = form_enabled.get();
        spawn_local_scoped(async move {
            match update_model(&model_name, &p, &upstream, enabled).await {
                Ok(_) => {
                    form_loading.set(false);
                    show_edit_modal.set(None);
                    model_refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    form_loading.set(false);
                    form_err.set(e.to_string());
                }
            }
        });
    };

    let options = provider_select_options(&providers);

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t(K::ModelEdit), move |_| show_edit_modal.set(None)),
                form_field(
                    "edit-model-name".into(),
                    i18n.t(K::Name),
                    input()
                        .attr("id", "edit-model-name")
                        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-600 text-gray-900 dark:text-gray-100 cursor-not-allowed")
                        .attr("type", "text")
                        .attr("value", model_name_for_display)
                        .attr("disabled", "")
                        .into(),
                ),
                form_field(
                    "edit-model-provider".into(),
                    i18n.t(K::Providers),
                    select_input("edit-model-provider".into(), form_provider_id, options),
                ),
                form_field(
                    "edit-model-upstream".into(),
                    i18n.t(K::ModelUpstreamModel),
                    form_input(
                        "edit-model-upstream".into(),
                        i18n.t(K::ModelUpstreamModel),
                        form_upstream,
                    ),
                ),
                form_checkbox(
                    "edit-enabled".into(),
                    i18n.t(K::StatusEnabled),
                    form_enabled,
                ),
                form_error(form_err),
                form_submit_footer(
                    i18n.t(K::Cancel),
                    move |_| show_edit_modal.set(None),
                    form_loading,
                    i18n.t(K::Save),
                ),
            )),
        move |_| show_edit_modal.set(None),
    )
}

fn make_model_rows(
    models: Vec<Model>,
    providers: &[Provider],
    show_detail: Signal<Option<usize>>,
    is_admin: Signal<bool>,
    show_delete_confirm: Signal<Option<Model>>,
    show_edit_modal: Signal<Option<Model>>,
) -> Vec<View> {
    let i18n = use_context::<I18n>();
    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);
    models
        .into_iter()
        .enumerate()
        .map(|(idx, m)| {
            let model_modal = m.clone();
            let show = show_detail;
            let prov_name = provider_name_by_id(&m.provider_id, providers);
            tr().class(zebra_bg(idx))
                .children((
                    name_cell(m.name),
                    secondary_cell(prov_name),
                    mono_cell(m.upstream_model),
                    text_cell(status_badge(m.enabled, &enabled_text, &disabled_text)),
                    timestamp_cell(m.updated_at),
                    action_cell(icon_button("fas fa-eye", move |_| show.set(Some(idx)))),
                    action_cell(
                        View::from_dynamic::<View>({
                            let show_edit = show_edit_modal;
                            let show_del = show_delete_confirm;
                            move || {
                                if is_admin.get() {
                                    let p_edit = model_modal.clone();
                                    let p_del = model_modal.clone();
                                    div().class("flex items-center justify-center gap-3").children((
                                        icon_button("fas fa-pen", move |_| show_edit.set(Some(p_edit.clone()))),
                                        icon_button("fas fa-trash", move |_| show_del.set(Some(p_del.clone()))),
                                    )).into()
                                } else {
                                    i().class("fas fa-ban text-gray-300 dark:text-gray-600 cursor-not-allowed").into()
                                }
                            }
                        }),
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
    let show_add_modal = create_signal(false);
    let models = props.models;
    let providers = props.providers;
    let is_admin = props.is_admin;
    let model_refresh = props.model_refresh;
    let model_refreshing = props.model_refreshing;

    let rows = make_model_rows(
        models.clone(),
        &providers,
        show_detail,
        is_admin,
        show_delete_confirm,
        show_edit_modal,
    );
    let count = models.len();

    let header = render_table_header(
        i18n.t(K::ModelTitle),
        count,
        model_refreshing,
        debounce_refresh(model_refresh, model_refreshing),
        render_add_button(is_admin, {
            let i18n = i18n.clone();
            move || {
                button()
                    .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer")
                    .on(events::click, {
                        move |_| show_add_modal.set(true)
                    })
                    .children((
                        i().class("fas fa-plus"),
                        span().children(i18n.t(K::ModelAdd)),
                    ))
                    .into()
            }
        }),
    );

    let table = table_shell(
        vec![
            th_left(i18n.t(K::Name)),
            th_left(i18n.t(K::Providers)),
            th_left(i18n.t(K::ModelUpstreamModel)),
            th_left(i18n.t(K::TableStatus)),
            th_left(i18n.t(K::UpdatedAt)),
            th_center(i18n.t(K::ProviderDetail)),
            th_center(i18n.t(K::ProviderActions)),
        ],
        rows,
    );

    let detail_modal = View::from_dynamic({
        let models = models.clone();
        let providers = providers.clone();
        move || match show_detail.get() {
            Some(idx) => models.get(idx).map_or(View::new(), |m| {
                render_model_detail(m.clone(), show_detail, &providers)
            }),
            None => View::new(),
        }
    });

    let add_modal = View::from_dynamic({
        let providers = providers.clone();
        move || {
            if show_add_modal.get() {
                render_add_modal(providers.clone(), model_refresh, show_add_modal)
            } else {
                View::new()
            }
        }
    });

    let edit_modal = View::from_dynamic({
        let providers = providers.clone();
        move || match show_edit_modal.get_clone() {
            Some(m) => {
                render_edit_modal(providers.clone(), model_refresh, show_edit_modal, m)
            }
            None => View::new(),
        }
    });

    let delete_modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_delete_confirm.get_clone() {
            Some(m) => {
                let deleting = create_signal(false);
                let model_name = m.name.clone();
                render_delete_confirm(
                    i18n.t_replace(K::DeleteConfirmMessage, "name", &m.name),
                    deleting,
                    move |_| {
                        if deleting.get() {
                            return;
                        }
                        deleting.set(true);
                        let mn = model_name.clone();
                        spawn_local_scoped(async move {
                            match delete_model(&mn).await {
                                Ok(_) => {
                                    model_refresh.update(|v| *v += 1);
                                }
                                Err(e) => {
                                    deleting.set(false);
                                    sycamore::web::console_log!("Failed: {}", e);
                                }
                            }
                        });
                    },
                    move |_| show_delete_confirm.set(None),
                )
            }
            None => View::new(),
        }
    });

    table_container(
        header,
        table,
        vec![detail_modal, add_modal, edit_modal, delete_modal],
    )
}
