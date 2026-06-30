use std::rc::Rc;

use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_model, delete_model, update_model};
use crate::components::delete_confirm::render_delete_confirm;
use crate::components::modal::{
    FormStatus, action_cell, blue_add_button, form_checkbox, form_error, form_field, form_input,
    form_submit_footer, icon_button, modal_dialog, modal_title, mono_cell, name_cell,
    render_detail_modal, secondary_cell, select_input, status_badge, text_cell, timestamp_cell,
};
use crate::components::table::{
    Column, CrudModal, common_table, debounce_refresh, th_center, th_left,
};
use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};
use crate::models::{Model, Provider};

struct ModelForm {
    name: Signal<String>,
    provider_id: Signal<String>,
    upstream: Signal<String>,
    enabled: Signal<bool>,
}

impl ModelForm {
    fn new(initial_provider_id: String) -> Self {
        Self {
            name: create_signal(String::new()),
            provider_id: create_signal(initial_provider_id),
            upstream: create_signal(String::new()),
            enabled: create_signal(true),
        }
    }

    fn from_model(m: &Model) -> Self {
        Self {
            name: create_signal(m.name.clone()),
            provider_id: create_signal(m.provider_id.clone()),
            upstream: create_signal(m.upstream_model.clone()),
            enabled: create_signal(m.enabled),
        }
    }
}

fn provider_name_by_id(provider_id: &str, providers: &[Provider]) -> String {
    providers
        .iter()
        .find(|p| p.id == provider_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| provider_id.to_string())
}

fn render_model_detail(
    model: Model,
    modal: Signal<CrudModal<Model>>,
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
        move |_| modal.set(CrudModal::Closed),
    )
}

fn provider_select_options(providers: &[Provider]) -> Vec<(String, String)> {
    providers
        .iter()
        .map(|p| (p.id.clone(), format!("{} ({})", p.name, p.provider_type)))
        .collect()
}

fn render_add_modal(
    providers: Rc<Vec<Provider>>,
    model_refresh: Signal<usize>,
    modal: Signal<CrudModal<Model>>,
) -> View {
    let i18n = use_context::<I18n>();
    let ModelForm {
        name: form_name,
        provider_id: form_provider_id,
        upstream: form_upstream,
        enabled: form_enabled,
    } = ModelForm::new(providers.first().map(|p| p.id.clone()).unwrap_or_default());
    let FormStatus {
        err: form_err,
        loading: form_loading,
    } = FormStatus::new();

    let close = move |_| modal.set(CrudModal::Closed);
    let i18n_err = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let p = form_provider_id.get_clone();
        if n.is_empty() || p.is_empty() {
            form_err.set(i18n_err.t(K::NameAndProviderIdRequired));
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
                    modal.set(CrudModal::Closed);
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
                modal_title(i18n.t_replace(K::Add, "entity", &i18n.t(K::Models)), close),
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
                form_submit_footer(i18n.t(K::Cancel), close, form_loading, i18n.t(K::Save)),
            )),
        close,
    )
}

fn render_edit_modal(
    providers: Rc<Vec<Provider>>,
    model_refresh: Signal<usize>,
    modal: Signal<CrudModal<Model>>,
    model: Model,
) -> View {
    let i18n = use_context::<I18n>();
    let model_name_for_display = model.name.clone();
    let ModelForm {
        name: form_name,
        provider_id: form_provider_id,
        upstream: form_upstream,
        enabled: form_enabled,
    } = ModelForm::from_model(&model);
    let FormStatus {
        err: form_err,
        loading: form_loading,
    } = FormStatus::new();

    let close = move |_| modal.set(CrudModal::Closed);
    let i18n_err = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let p = form_provider_id.get_clone();
        if n.is_empty() || p.is_empty() {
            form_err.set(i18n_err.t(K::NameAndProviderIdRequired));
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
                    modal.set(CrudModal::Closed);
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
                modal_title(i18n.t_replace(K::Edit, "entity", &i18n.t(K::Models)), close),
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
                    close,
                    form_loading,
                    i18n.t(K::Save),
                ),
            )),
        close,
    )
}

#[derive(Props)]
pub struct ModelTableProps {
    pub models: Vec<Model>,
    pub providers: Vec<Provider>,
    pub model_refresh: Signal<usize>,
    pub model_refreshing: Signal<bool>,
}

#[component]
pub fn ModelTable(props: ModelTableProps) -> View {
    let i18n = use_context::<I18n>();
    let toast = use_context::<ToastManager>();
    let modal = create_signal::<CrudModal<Model>>(CrudModal::Closed);
    let deleting = create_signal(false);
    let models = props.models;
    let providers = Rc::new(props.providers);
    let model_refresh = props.model_refresh;
    let model_refreshing = props.model_refreshing;

    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);

    let columns = vec![
        Column {
            header: th_left(i18n.t(K::Name)),
            cell: Rc::new(|m: Model| name_cell(m.name)),
        },
        Column {
            header: th_left(i18n.t(K::Providers)),
            cell: Rc::new({
                let p = Rc::clone(&providers);
                move |m: Model| secondary_cell(provider_name_by_id(&m.provider_id, &p))
            }),
        },
        Column {
            header: th_left(i18n.t(K::ModelUpstreamModel)),
            cell: Rc::new(|m: Model| mono_cell(m.upstream_model)),
        },
        Column {
            header: th_left(i18n.t(K::TableStatus)),
            cell: {
                let et = enabled_text.clone();
                let dt = disabled_text.clone();
                Rc::new(move |m: Model| text_cell(status_badge(m.enabled, &et, &dt)))
            },
        },
        Column {
            header: th_left(i18n.t(K::UpdatedAt)),
            cell: Rc::new(|m: Model| timestamp_cell(m.updated_at)),
        },
        Column {
            header: th_center(i18n.t(K::Detail)),
            cell: Rc::new(move |m: Model| {
                action_cell(icon_button("fas fa-eye", move |_| {
                    modal.set(CrudModal::Detail(m.clone()))
                }))
            }),
        },
        Column {
            header: th_center(i18n.t(K::Actions)),
            cell: Rc::new(move |m: Model| {
                let p1 = m.clone();
                let p2 = m.clone();
                action_cell(
                    div()
                        .class("flex items-center justify-center gap-3")
                        .children((
                            icon_button("fas fa-pen", move |_| {
                                modal.set(CrudModal::Edit(p1.clone()))
                            }),
                            icon_button("fas fa-trash", move |_| {
                                modal.set(CrudModal::Delete(p2.clone()))
                            }),
                        )),
                )
            }),
        },
    ];

    let add_button = blue_add_button(
        i18n.t_replace(K::Add, "entity", &i18n.t(K::Models)),
        move |_| modal.set(CrudModal::Add),
    );

    let i18n_m = i18n.clone();
    let providers_m = Rc::clone(&providers);
    let modals = View::from_dynamic(move || match modal.get_clone() {
        CrudModal::Detail(m) => render_model_detail(m, modal, &providers_m),
        CrudModal::Add => render_add_modal(Rc::clone(&providers_m), model_refresh, modal),
        CrudModal::Edit(m) => render_edit_modal(Rc::clone(&providers_m), model_refresh, modal, m),
        CrudModal::Delete(m) => render_delete_confirm(
            i18n_m.t_replace(K::DeleteConfirmMessage, "name", &m.name),
            deleting,
            {
                let toast = toast.clone();
                let i18n = i18n_m.clone();
                move |_| {
                    if deleting.get() {
                        return;
                    }
                    deleting.set(true);
                    let mn = m.name.clone();
                    let toast = toast.clone();
                    let i18n = i18n.clone();
                    spawn_local_scoped(async move {
                        match delete_model(&mn).await {
                            Ok(_) => {
                                toast.success(i18n.t(K::ModelDeleted));
                                model_refresh.update(|v| *v += 1);
                            }
                            Err(e) => {
                                deleting.set(false);
                                toast.error(e.to_string());
                            }
                        }
                    });
                }
            },
            move |_| modal.set(CrudModal::Closed),
        ),
        CrudModal::Closed => {
            deleting.set(false);
            View::new()
        }
    });

    common_table(
        i18n.t(K::ModelTitle),
        models,
        model_refreshing,
        debounce_refresh(model_refresh, model_refreshing),
        columns,
        add_button,
        vec![modals],
    )
}
