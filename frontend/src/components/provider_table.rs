use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;
use web_sys::wasm_bindgen::JsCast;

use crate::api::{create_provider, delete_provider, update_provider};
use crate::components::modal::{
    detail_row, form_checkbox, form_delete_footer, form_error, form_field, form_input,
    form_submit_footer, modal_dialog, modal_title,
};
use crate::i18n::I18n;
use crate::models::Provider;

fn render_detail_modal(i18n: &I18n, prov: Provider, show_detail: Signal<Option<usize>>) -> View {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    let status = if prov.enabled {
        enabled_text
    } else {
        disabled_text
    };
    let api_key_display = prov.api_key.clone().unwrap_or_else(|| "—".to_string());

    modal_dialog(
        (
            modal_title(
                i18n.t_replace("detail_title", "entity", &i18n.t("providers")),
                move |_| show_detail.set(None),
            ),
            detail_row("ID".to_string(), prov.id),
            detail_row(i18n.t("name"), prov.name),
            detail_row(i18n.t("provider_api_type"), prov.provider_type),
            detail_row(i18n.t("provider_base_url"), prov.base_url),
            detail_row(i18n.t("api_key"), api_key_display),
            detail_row(i18n.t("table_status"), status),
            detail_row(i18n.t("created_at"), format!("{}", prov.created_at as u64)),
            detail_row(i18n.t("updated_at"), format!("{}", prov.updated_at as u64)),
        ),
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
            form_err.set("Name and Base URL are required".to_string());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let name = n;
        let ptype = form_type.get_clone();
        let base_url = u;
        let api_key = {
            let raw = form_api_key.get_clone();
            if raw.is_empty() { None } else { Some(raw) }
        };
        let enabled = form_enabled.get();
        let refresh = provider_refresh;
        let loading = form_loading;
        spawn_local_scoped(async move {
            match create_provider(&name, &ptype, &base_url, api_key, enabled).await {
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

    modal_dialog(
        form()
            .on(events::submit, on_submit)
            .class("space-y-4")
            .children((
                modal_title(i18n.t("provider_add"), move |_| show_add_modal.set(false)),
                form_field(i18n.t("name"), form_input(i18n.t("name"), form_name)),
                form_field(i18n.t("provider_api_type"),
                    select()
                        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                        .bind(bind::value, form_type)
                        .children((
                            option().attr("value", "openai_compat").children("OpenAI Compatible"),
                            option().attr("value", "deepseek").children("DeepSeek"),
                            option().attr("value", "zhipu").children("Zhipu"),
                            option().attr("value", "ollama").children("Ollama"),
                            option().attr("value", "llamacpp").children("llama.cpp"),
                        )).into()),
                form_field(i18n.t("provider_base_url"), form_input(i18n.t("provider_base_url"), form_base_url)),
                form_field(i18n.t("api_key"), form_input(i18n.t("api_key"), form_api_key)),
                form_checkbox("add-enabled".to_string(), i18n.t("status_enabled"), form_enabled),
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
            form_err.set("Name and Base URL are required".to_string());
            return;
        }
        form_loading.set(true);
        form_err.set(String::new());
        let pid = prov.id.clone();
        let name = n;
        let ptype = form_type.get_clone();
        let base_url = u;
        let api_key = {
            let raw = form_api_key.get_clone();
            if raw.is_empty() { None } else { Some(raw) }
        };
        let enabled = form_enabled.get();
        let refresh = provider_refresh;
        let loading = form_loading;
        let err = form_err;
        spawn_local_scoped(async move {
            match update_provider(&pid, &name, &ptype, &base_url, api_key, enabled).await {
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
                form_field(i18n.t("provider_api_type"),
                    select()
                        .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                        .on(events::change, move |ev: web_sys::Event| {
                            form_type.set(ev.target().unwrap().unchecked_into::<web_sys::HtmlSelectElement>().value());
                        })
                        .children((
                            option().attr("value", "openai_compat")
                                .bool_attr("selected", move || form_type.get_clone() == "openai_compat")
                                .children("OpenAI Compatible"),
                            option().attr("value", "deepseek")
                                .bool_attr("selected", move || form_type.get_clone() == "deepseek")
                                .children("DeepSeek"),
                            option().attr("value", "zhipu")
                                .bool_attr("selected", move || form_type.get_clone() == "zhipu")
                                .children("Zhipu"),
                            option().attr("value", "ollama")
                                .bool_attr("selected", move || form_type.get_clone() == "ollama")
                                .children("Ollama"),
                            option().attr("value", "llamacpp")
                                .bool_attr("selected", move || form_type.get_clone() == "llamacpp")
                                .children("llama.cpp"),
                        )).into()),
                form_field(i18n.t("provider_base_url"), form_input(i18n.t("provider_base_url"), form_base_url)),
                div().children((
                    label().class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                        .children(i18n.t("api_key")),
                    form_input(i18n.t("api_key"), form_api_key),
                    p().class("text-xs text-gray-400 dark:text-gray-500 mt-1")
                        .children("Leave empty to keep current key"),
                )),
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
    provider_refresh: Signal<usize>,
    show_delete_confirm: Signal<Option<Provider>>,
    prov: Provider,
) -> View {
    let deleting = create_signal(false);
    let prov_id = prov.id.clone();
    let on_delete = move |_| {
        if deleting.get() {
            return;
        }
        deleting.set(true);
        let pid = prov_id.clone();
        let refresh = provider_refresh;
        let d = deleting;
        spawn_local_scoped(async move {
            match delete_provider(&pid).await {
                Ok(_) => {
                    refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    d.set(false);
                    sycamore::web::console_log!("Failed to delete provider: {}", e);
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
                .children(i18n.t_replace("delete_confirm_message", "name", &prov.name)),
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
            let ec = if prov.enabled {
                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
            } else {
                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
            };
            let st = if prov.enabled { &enabled_text } else { &disabled_text };
            let bg = if idx % 2 == 0 { "" } else { "bg-gray-50 dark:bg-gray-800/50" };
            let span_class = format!("inline-block px-2 py-1 rounded-full text-xs font-medium {}", ec);
            let show = show_detail;
            tr().class(bg)
                .children((
                    td().class("px-6 py-4 font-medium text-gray-800 dark:text-gray-200")
                        .children(prov.name),
                    td().class("px-6 py-4 text-gray-600 dark:text-gray-400")
                        .children(prov.provider_type),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono")
                        .children(prov.base_url),
                    td().class("px-6 py-4")
                        .children(span().class(span_class).children(st.clone())),
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
                                            let p = prov_modal.clone();
                                            move |_| show_edit_modal.set(Some(p.clone()))
                                        })
                                        .children(i().class("fas fa-pen text-xs")),
                                    button()
                                        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                        .on(events::click, {
                                            let p = prov_modal.clone();
                                            move |_| show_delete_confirm.set(Some(p.clone()))
                                        })
                                        .children(i().class("fas fa-trash text-xs")),
                                )).into()
                            } else {
                                i().class("fas fa-ban text-gray-300 dark:text-gray-600 cursor-not-allowed").into()
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

    let modal = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_detail.get() {
            Some(idx) => providers.get(idx).map_or(View::new(), |prov| {
                render_detail_modal(&i18n, prov.clone(), show_detail)
            }),
            None => View::new(),
        }
    });

    let show_add_modal = create_signal(false);
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

    let delete_confirm = View::from_dynamic({
        let i18n = i18n.clone();
        move || match show_delete_confirm.get_clone() {
            Some(prov) => render_delete_confirm(&i18n, provider_refresh, show_delete_confirm, prov),
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
                            .children(i18n.t("provider_title")),
                        span().class(
                            "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                        )
                        .children(i18n.t_replace("total_count", "count", &count.to_string())),
                        button()
                            .disabled(move || provider_refreshing.get())
                            .class("text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed")
                            .on(events::click, move |_| {
                                if provider_refreshing.get() { return; }
                                provider_refreshing.set(true);
                                let r = provider_refresh;
                                Timeout::new(50, move || { r.update(|v| *v += 1); }).forget();
                            })
                            .children(i().class(move || {
                                if provider_refreshing.get() { "fas fa-sync-alt animate-spin" } else { "fas fa-sync-alt" }
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
                                        span().children(i18n.t("provider_add")),
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
                                    .children(i18n.t("provider_api_type")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("provider_base_url")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("table_status")),
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
            edit_modal,
            delete_confirm,
        ))
        .into()
}
