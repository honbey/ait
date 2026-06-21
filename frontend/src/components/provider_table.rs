use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_provider, delete_provider, update_provider};
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
use crate::i18n::I18n;
use crate::models::Provider;

fn provider_type_options() -> Vec<(String, String)> {
    vec![
        ("openai_compat".into(), "OpenAI Compatible".into()),
        ("deepseek".into(), "DeepSeek".into()),
        ("zhipu".into(), "Zhipu".into()),
        ("ollama".into(), "Ollama".into()),
        ("llamacpp".into(), "llama.cpp".into()),
    ]
}

fn render_provider_detail(i18n: &I18n, prov: Provider, show_detail: Signal<Option<usize>>) -> View {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    let status = if prov.enabled {
        enabled_text
    } else {
        disabled_text
    };
    let api_key_display = prov.api_key.clone().unwrap_or_else(|| "—".to_string());

    render_detail_modal(
        i18n.t_replace("detail_title", "entity", &i18n.t("providers")),
        vec![
            ("ID".into(), prov.id),
            (i18n.t("name"), prov.name),
            (i18n.t("provider_api_type"), prov.provider_type),
            (i18n.t("provider_base_url"), prov.base_url),
            (i18n.t("api_key"), api_key_display),
            (i18n.t("table_status"), status),
            (
                i18n.t("created_at"),
                crate::models::format_timestamp(prov.created_at),
            ),
            (
                i18n.t("updated_at"),
                crate::models::format_timestamp(prov.updated_at),
            ),
        ],
        move |_| show_detail.set(None),
    )
}

fn render_add_modal(
    i18n: &I18n,
    provider_refresh: Signal<usize>,
    show_add_modal: Signal<bool>,
) -> View {
    let form_name = create_signal(String::new());
    let form_type = create_signal("openai_compat".to_string());
    let form_base_url = create_signal(String::new());
    let form_api_key = create_signal(String::new());
    let form_enabled = create_signal(true);
    let form_err = create_signal(String::new());
    let form_loading = create_signal(false);

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
        let api_key = {
            let raw = form_api_key.get_clone();
            if raw.is_empty() { None } else { Some(raw) }
        };
        let ptype = form_type.get_clone();
        let enabled = form_enabled.get();
        let refresh = provider_refresh;
        let loading = form_loading;
        let err = form_err;
        spawn_local_scoped(async move {
            match create_provider(&n, &ptype, &u, api_key, enabled).await {
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

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t("provider_add"), move |_| show_add_modal.set(false)),
                form_field(i18n.t("name"), form_input(i18n.t("name"), form_name)),
                form_field(
                    i18n.t("provider_api_type"),
                    select_input(form_type, provider_type_options()),
                ),
                form_field(
                    i18n.t("provider_base_url"),
                    form_input(i18n.t("provider_base_url"), form_base_url),
                ),
                form_field(
                    i18n.t("api_key"),
                    form_input(i18n.t("api_key"), form_api_key),
                ),
                form_checkbox("add-enabled".into(), i18n.t("status_enabled"), form_enabled),
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
    provider_refresh: Signal<usize>,
    show_edit_modal: Signal<Option<Provider>>,
    prov: Provider,
) -> View {
    let form_name = create_signal(prov.name.clone());
    let form_type = create_signal(prov.provider_type.clone());
    let form_base_url = create_signal(prov.base_url.clone());
    let form_api_key = create_signal(String::new());
    let form_enabled = create_signal(prov.enabled);
    let form_err = create_signal(String::new());
    let form_loading = create_signal(false);

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
            if raw.is_empty() { None } else { Some(raw) }
        };
        let refresh = provider_refresh;
        let loading = form_loading;
        let err = form_err;
        let ptype = form_type.get_clone();
        let enabled = form_enabled.get();
        spawn_local_scoped(async move {
            match update_provider(&pid, &n, &ptype, &u, api_key, enabled).await {
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

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t("provider_edit"), move |_| show_edit_modal.set(None)),
                form_field(i18n.t("name"), form_input(i18n.t("name"), form_name)),
                form_field(
                    i18n.t("provider_api_type"),
                    select_input(form_type, provider_type_options()),
                ),
                form_field(
                    i18n.t("provider_base_url"),
                    form_input(i18n.t("provider_base_url"), form_base_url),
                ),
                div().children((
                    label()
                        .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                        .children(i18n.t("api_key")),
                    form_input(i18n.t("api_key"), form_api_key),
                    p().class("text-xs text-gray-400 dark:text-gray-500 mt-1")
                        .children("Leave empty to keep current key"),
                )),
                form_checkbox(
                    "edit-enabled".into(),
                    i18n.t("status_enabled"),
                    form_enabled,
                ),
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

fn make_provider_rows(
    providers: Vec<Provider>,
    i18n: &I18n,
    show_detail: Signal<Option<usize>>,
    is_admin: Signal<bool>,
    show_edit_modal: Signal<Option<Provider>>,
    show_delete_confirm: Signal<Option<Provider>>,
) -> Vec<View> {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    providers
        .into_iter()
        .enumerate()
        .map(|(idx, prov)| {
            let prov_modal = prov.clone();
            let show = show_detail;
            tr().class(zebra_bg(idx))
                .children((
                    name_cell(prov.name),
                    secondary_cell(prov.provider_type),
                    mono_cell(prov.base_url),
                    text_cell(status_badge(prov.enabled, &enabled_text, &disabled_text)),
                    timestamp_cell(prov.updated_at),
                    action_cell(icon_button("fas fa-eye", move |_| show.set(Some(idx)))),
                    action_cell(
                        View::from_dynamic::<View>({
                            let show_edit = show_edit_modal;
                            let show_del = show_delete_confirm;
                            move || {
                                if is_admin.get() {
                                    let p_edit = prov_modal.clone();
                                    let p_del = prov_modal.clone();
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
pub struct ProviderTableProps {
    pub providers: Vec<Provider>,
    pub is_admin: Signal<bool>,
    pub provider_refresh: Signal<usize>,
    pub provider_refreshing: Signal<bool>,
}

#[component]
pub fn ProviderTable(props: ProviderTableProps) -> View {
    let i18n = use_context::<I18n>();
    let show_detail = create_signal::<Option<usize>>(None);
    let show_edit_modal = create_signal::<Option<Provider>>(None);
    let show_delete_confirm = create_signal::<Option<Provider>>(None);
    let show_add_modal = create_signal(false);
    let providers = props.providers;
    let is_admin = props.is_admin;
    let provider_refresh = props.provider_refresh;
    let provider_refreshing = props.provider_refreshing;

    let rows = make_provider_rows(
        providers.clone(),
        &i18n,
        show_detail,
        is_admin,
        show_edit_modal,
        show_delete_confirm,
    );
    let count = providers.len();

    let header = render_table_header(
        &i18n,
        i18n.t("provider_title"),
        count,
        provider_refreshing,
        debounce_refresh(provider_refresh, provider_refreshing),
        render_add_button(is_admin, {
            let i18n = i18n.clone();
            move || {
                button()
                    .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer")
                    .on(events::click, {
                        let sam = show_add_modal;
                        move |_| sam.set(true)
                    })
                    .children((
                        i().class("fas fa-plus"),
                        span().children(i18n.t("provider_add")),
                    ))
                    .into()
            }
        }),
    );

    let table = table_shell(
        vec![
            th_left(i18n.t("name")),
            th_left(i18n.t("provider_api_type")),
            th_left(i18n.t("provider_base_url")),
            th_left(i18n.t("table_status")),
            th_left(i18n.t("updated_at")),
            th_center(i18n.t("provider_detail")),
            th_center(i18n.t("provider_actions")),
        ],
        rows,
    );

    let detail_modal = View::from_dynamic({
        let i18n = i18n.clone();
        let providers = providers.clone();
        move || match show_detail.get() {
            Some(idx) => providers.get(idx).map_or(View::new(), |prov| {
                render_provider_detail(&i18n, prov.clone(), show_detail)
            }),
            None => View::new(),
        }
    });

    let add_modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || {
            if show_add_modal.get() {
                render_add_modal(&i18n, provider_refresh, show_add_modal)
            } else {
                View::new()
            }
        }
    });

    let edit_modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_edit_modal.get_clone() {
            Some(prov) => render_edit_modal(&i18n, provider_refresh, show_edit_modal, prov),
            None => View::new(),
        }
    });

    let delete_modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_delete_confirm.get_clone() {
            Some(prov) => {
                let deleting = create_signal(false);
                let prov_id = prov.id.clone();
                let refresh = provider_refresh;
                render_delete_confirm(
                    &i18n,
                    i18n.t_replace("delete_confirm_message", "name", &prov.name),
                    deleting,
                    move |_| {
                        if deleting.get() {
                            return;
                        }
                        deleting.set(true);
                        let pid = prov_id.clone();
                        let d = deleting;
                        spawn_local_scoped(async move {
                            match delete_provider(&pid).await {
                                Ok(_) => {
                                    refresh.update(|v| *v += 1);
                                }
                                Err(e) => {
                                    d.set(false);
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
