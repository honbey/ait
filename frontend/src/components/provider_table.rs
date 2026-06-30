use std::rc::Rc;

use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_provider, delete_provider, fetch_provider_types, update_provider};
use crate::components::delete_confirm::render_delete_confirm;
use crate::components::modal::{
    FormStatus, action_cell, blue_add_button, form_checkbox, form_error, form_field,
    form_field_with_hint, form_input, form_submit_footer, icon_button, modal_dialog, modal_title,
    mono_cell, name_cell, render_detail_modal, secondary_cell, select_input, status_badge,
    text_cell, timestamp_cell,
};
use crate::components::table::{
    Column, CrudModal, common_table, debounce_refresh, th_center, th_left,
};
use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};
use crate::models::Provider;
use crate::storage::get_storage;

struct ProviderForm {
    name: Signal<String>,
    provider_type: Signal<String>,
    base_url: Signal<String>,
    api_key: Signal<String>,
    enabled: Signal<bool>,
}

impl ProviderForm {
    fn new() -> Self {
        Self {
            name: create_signal(String::new()),
            provider_type: create_signal("openai_compat".to_string()),
            base_url: create_signal(String::new()),
            api_key: create_signal(String::new()),
            enabled: create_signal(true),
        }
    }

    fn from_provider(p: &Provider) -> Self {
        Self {
            name: create_signal(p.name.clone()),
            provider_type: create_signal(p.provider_type.clone()),
            base_url: create_signal(p.base_url.clone()),
            api_key: create_signal(String::new()),
            enabled: create_signal(p.enabled),
        }
    }
}

fn provider_display_name(provider_type: &str, provider_types: &[(String, String)]) -> String {
    provider_types
        .iter()
        .find(|(id, _)| id == provider_type)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| provider_type.to_string())
}

fn render_provider_detail(
    prov: Provider,
    modal: Signal<CrudModal<Provider>>,
    provider_types: &[(String, String)],
) -> View {
    let i18n = use_context::<I18n>();
    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);
    let status = if prov.enabled {
        enabled_text
    } else {
        disabled_text
    };
    let api_key_display = prov.api_key.clone().unwrap_or_else(|| "—".to_string());

    render_detail_modal(
        i18n.t_replace(K::DetailTitle, "entity", &i18n.t(K::Providers)),
        vec![
            ("ID".into(), prov.id),
            (i18n.t(K::Name), prov.name),
            (
                i18n.t(K::Providers),
                provider_display_name(&prov.provider_type, provider_types),
            ),
            (i18n.t(K::ProviderBaseUrl), prov.base_url),
            (i18n.t(K::ApiKey), api_key_display),
            (i18n.t(K::TableStatus), status),
            (
                i18n.t(K::CreatedAt),
                crate::models::format_timestamp(prov.created_at),
            ),
            (
                i18n.t(K::UpdatedAt),
                crate::models::format_timestamp(prov.updated_at),
            ),
        ],
        move |_| modal.set(CrudModal::Closed),
    )
}

fn render_add_modal(
    provider_refresh: Signal<usize>,
    modal: Signal<CrudModal<Provider>>,
    provider_types: Vec<(String, String)>,
) -> View {
    let i18n = use_context::<I18n>();
    let ProviderForm {
        name: form_name,
        provider_type: form_type,
        base_url: form_base_url,
        api_key: form_api_key,
        enabled: form_enabled,
    } = ProviderForm::new();
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
        let u = form_base_url.get_clone();
        if n.is_empty() || u.is_empty() {
            form_err.set(i18n_err.t(K::NameAndBaseUrlRequired));
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let api_key = {
            let raw = form_api_key.get_clone();
            if raw.is_empty() { None } else { Some(raw) }
        };
        let ptype = form_type.get_clone();
        let enabled = form_enabled.get();
        spawn_local_scoped(async move {
            match create_provider(&n, &ptype, &u, api_key, enabled).await {
                Ok(_) => {
                    form_loading.set(false);
                    modal.set(CrudModal::Closed);
                    provider_refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    form_loading.set(false);
                    form_err.set(e.to_string());
                }
            }
        });
    };

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(
                    i18n.t_replace(K::Add, "entity", &i18n.t(K::Providers)),
                    close,
                ),
                form_field(
                    "add-provider-name".into(),
                    i18n.t(K::Name),
                    form_input("add-provider-name".into(), i18n.t(K::Name), form_name),
                ),
                form_field(
                    "add-provider-type".into(),
                    i18n.t(K::Providers),
                    select_input("add-provider-type".into(), form_type, provider_types),
                ),
                form_field(
                    "add-provider-url".into(),
                    i18n.t(K::ProviderBaseUrl),
                    form_input(
                        "add-provider-url".into(),
                        i18n.t(K::ProviderBaseUrl),
                        form_base_url,
                    ),
                ),
                form_field(
                    "add-provider-apikey".into(),
                    i18n.t(K::ApiKey),
                    form_input(
                        "add-provider-apikey".into(),
                        i18n.t(K::ApiKey),
                        form_api_key,
                    ),
                ),
                form_checkbox("add-enabled".into(), i18n.t(K::StatusEnabled), form_enabled),
                form_error(form_err),
                form_submit_footer(i18n.t(K::Cancel), close, form_loading, i18n.t(K::Save)),
            )),
        close,
    )
}

fn render_edit_modal(
    provider_refresh: Signal<usize>,
    modal: Signal<CrudModal<Provider>>,
    prov: Provider,
    provider_types: Vec<(String, String)>,
) -> View {
    let i18n = use_context::<I18n>();
    let ProviderForm {
        name: form_name,
        provider_type: form_type,
        base_url: form_base_url,
        api_key: form_api_key,
        enabled: form_enabled,
    } = ProviderForm::from_provider(&prov);
    let FormStatus {
        err: form_err,
        loading: form_loading,
    } = FormStatus::new();
    let form_clear_key = create_signal(false);

    let close = move |_| modal.set(CrudModal::Closed);
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let u = form_base_url.get_clone();
        if n.is_empty() || u.is_empty() {
            form_err.set("Name and Base URL are required".into());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let pid = prov.id.clone();
        let api_key = {
            let raw = form_api_key.get_clone();
            if !raw.is_empty() {
                Some(raw)
            } else if form_clear_key.get() {
                Some(String::new())
            } else {
                None
            }
        };
        let ptype = form_type.get_clone();
        let enabled = form_enabled.get();
        spawn_local_scoped(async move {
            match update_provider(&pid, &n, &ptype, &u, api_key, enabled).await {
                Ok(_) => {
                    form_loading.set(false);
                    modal.set(CrudModal::Closed);
                    provider_refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    form_loading.set(false);
                    form_err.set(e.to_string());
                }
            }
        });
    };

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(
                    i18n.t_replace(K::Edit, "entity", &i18n.t(K::Providers)),
                    close,
                ),
                form_field(
                    "edit-provider-name".into(),
                    i18n.t(K::Name),
                    form_input("edit-provider-name".into(), i18n.t(K::Name), form_name),
                ),
                form_field(
                    "edit-provider-type".into(),
                    i18n.t(K::Providers),
                    select_input("edit-provider-type".into(), form_type, provider_types),
                ),
                form_field(
                    "edit-provider-url".into(),
                    i18n.t(K::ProviderBaseUrl),
                    form_input(
                        "edit-provider-url".into(),
                        i18n.t(K::ProviderBaseUrl),
                        form_base_url,
                    ),
                ),
                form_field_with_hint(
                    "edit-provider-apikey".into(),
                    i18n.t(K::ApiKey),
                    i18n.t(K::KeepKeyHint),
                    form_input(
                        "edit-provider-apikey".into(),
                        i18n.t(K::ApiKey),
                        form_api_key,
                    ),
                ),
                form_checkbox(
                    "edit-provider-clear-key".into(),
                    i18n.t(K::ClearKey),
                    form_clear_key,
                ),
                form_checkbox(
                    "edit-enabled".into(),
                    i18n.t(K::StatusEnabled),
                    form_enabled,
                ),
                form_error(form_err),
                form_submit_footer(i18n.t(K::Cancel), close, form_loading, i18n.t(K::Save)),
            )),
        close,
    )
}

#[derive(Props)]
pub struct ProviderTableProps {
    pub providers: Vec<Provider>,
    pub provider_refresh: Signal<usize>,
    pub provider_refreshing: Signal<bool>,
}

#[component]
pub fn ProviderTable(props: ProviderTableProps) -> View {
    let i18n = use_context::<I18n>();
    let toast = use_context::<ToastManager>();
    let modal = create_signal::<CrudModal<Provider>>(CrudModal::Closed);
    let deleting = create_signal(false);
    let providers = props.providers;
    let provider_refresh = props.provider_refresh;
    let provider_refreshing = props.provider_refreshing;

    let storage = get_storage();
    const PT_KEY: &str = "ait_provider_types";
    const PT_TS_KEY: &str = "ait_provider_types_ts";
    const PT_TTL_MS: f64 = 3_600_000.0; // 1 hour

    let cached_fresh = storage
        .get_item(PT_TS_KEY)
        .and_then(|ts| ts.parse::<f64>().ok())
        .map(|ts| js_sys::Date::now() - ts < PT_TTL_MS)
        .unwrap_or(false);

    let provider_types = create_signal(if cached_fresh {
        storage
            .get_item(PT_KEY)
            .and_then(|json| serde_json::from_str::<Vec<(String, String)>>(&json).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    });

    if provider_types.get_clone().is_empty() {
        let st = storage;
        spawn_local_scoped(async move {
            if let Ok(types) = fetch_provider_types().await {
                let pairs: Vec<(String, String)> = types
                    .into_iter()
                    .map(|t| (t.provider_type, t.display_name))
                    .collect();
                if let Ok(json) = serde_json::to_string(&pairs) {
                    st.set_item(PT_KEY, &json);
                    st.set_item(PT_TS_KEY, &js_sys::Date::now().to_string());
                }
                provider_types.set(pairs);
            }
        });
    }

    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);

    let columns = vec![
        Column {
            header: th_left(i18n.t(K::Name)),
            cell: Rc::new(|p: Provider| name_cell(p.name)),
        },
        Column {
            header: th_left(i18n.t(K::Providers)),
            cell: Rc::new({
                let pt = provider_types;
                move |p: Provider| {
                    secondary_cell(provider_display_name(&p.provider_type, &pt.get_clone()))
                }
            }),
        },
        Column {
            header: th_left(i18n.t(K::ProviderBaseUrl)),
            cell: Rc::new(|p: Provider| mono_cell(p.base_url)),
        },
        Column {
            header: th_left(i18n.t(K::TableStatus)),
            cell: {
                let et = enabled_text.clone();
                let dt = disabled_text.clone();
                Rc::new(move |p: Provider| text_cell(status_badge(p.enabled, &et, &dt)))
            },
        },
        Column {
            header: th_left(i18n.t(K::UpdatedAt)),
            cell: Rc::new(|p: Provider| timestamp_cell(p.updated_at)),
        },
        Column {
            header: th_center(i18n.t(K::Detail)),
            cell: Rc::new(move |p: Provider| {
                action_cell(icon_button("fas fa-eye", move |_| {
                    modal.set(CrudModal::Detail(p.clone()))
                }))
            }),
        },
        Column {
            header: th_center(i18n.t(K::Actions)),
            cell: Rc::new(move |p: Provider| {
                let p1 = p.clone();
                let p2 = p.clone();
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
        i18n.t_replace(K::Add, "entity", &i18n.t(K::Providers)),
        move |_| modal.set(CrudModal::Add),
    );

    let i18n_modals = i18n.clone();
    let modals = View::from_dynamic(move || match modal.get_clone() {
        CrudModal::Detail(prov) => render_provider_detail(prov, modal, &provider_types.get_clone()),
        CrudModal::Add => render_add_modal(provider_refresh, modal, provider_types.get_clone()),
        CrudModal::Edit(prov) => {
            render_edit_modal(provider_refresh, modal, prov, provider_types.get_clone())
        }
        CrudModal::Delete(prov) => render_delete_confirm(
            i18n_modals.t_replace(K::DeleteConfirmMessage, "name", &prov.name),
            deleting,
            {
                let toast = toast.clone();
                let i18n = i18n_modals.clone();
                move |_| {
                    if deleting.get() {
                        return;
                    }
                    deleting.set(true);
                    let pid = prov.id.clone();
                    let toast = toast.clone();
                    let i18n = i18n.clone();
                    spawn_local_scoped(async move {
                        match delete_provider(&pid).await {
                            Ok(_) => {
                                toast.success(i18n.t(K::ProviderDeleted));
                                provider_refresh.update(|v| *v += 1);
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
        i18n.t(K::ProviderTitle),
        providers,
        provider_refreshing,
        debounce_refresh(provider_refresh, provider_refreshing),
        columns,
        add_button,
        vec![modals],
    )
}
