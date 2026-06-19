use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;
use gloo_timers::callback::Timeout;

use crate::api::create_provider;
use crate::i18n::I18n;
use crate::models::Provider;

fn detail_row(label: String, value: String) -> View {
    div()
        .class("flex justify-between items-center py-2 border-b border-gray-100 dark:border-gray-700 last:border-0")
        .children((
            span().class("text-gray-500 dark:text-gray-400 text-sm").children(label),
            span().class("text-gray-900 dark:text-gray-100 font-medium text-sm text-right ml-4 truncate").children(value),
        ))
        .into()
}

fn render_detail_modal(i18n: &I18n, prov: Provider, show_detail: Signal<Option<usize>>) -> View {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    let status = if prov.enabled {
        enabled_text
    } else {
        disabled_text
    };
    let api_key_display = prov.api_key.clone().unwrap_or_else(|| "—".to_string());

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
                                .children(i18n.t_replace("detail_title", "entity", &i18n.t("providers"))),
                            button()
                                .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                .on(events::click, move |_| show_detail.set(None))
                                .children(i().class("fas fa-times")),
                        )),
                    detail_row("ID".to_string(), prov.id),
                    detail_row(i18n.t("name"), prov.name),
                    detail_row(i18n.t("provider_api_type"), prov.provider_type),
                    detail_row(i18n.t("provider_base_url"), prov.base_url),
                    detail_row(i18n.t("api_key"), api_key_display),
                    detail_row(i18n.t("table_status"), status),
                    detail_row(i18n.t("created_at"), format!("{}", prov.created_at as u64)),
                    detail_row(i18n.t("updated_at"), format!("{}", prov.updated_at as u64)),
                )),
        ))
        .into()
}

fn render_add_modal(
    i18n: &I18n,
    provider_refresh: sycamore::reactive::Signal<usize>,
    show_add_modal: sycamore::reactive::Signal<bool>,
) -> View {
    let form_name = create_signal(String::new());
    let form_type = create_signal("open_ai_compat".to_string());
    let form_base_url = create_signal(String::new());
    let form_api_key = create_signal(String::new());
    let form_enabled = create_signal(true);
    let form_error = create_signal(String::new());
    let form_loading = create_signal(false);

    let i18n_save = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if form_loading.get() {
            return;
        }
        let n = form_name.get_clone();
        let u = form_base_url.get_clone();
        if n.is_empty() || u.is_empty() {
            form_error.set("Name and Base URL are required".to_string());
            return;
        }
        form_loading.set(true);
        form_error.set(String::new());

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
                    form_error.set(e.to_string());
                }
            }
        });
    };

    div()
        .class("fixed inset-0 z-50 flex items-center justify-center")
        .children((
            div()
                .class("absolute inset-0 bg-black/50")
                .on(events::click, move |_| show_add_modal.set(false)),
            div()
                .class("relative z-10 bg-white dark:bg-gray-800 rounded-xl p-6 shadow-2xl max-w-md w-full mx-4")
                .children(
                    form()
                        .on(events::submit, on_submit)
                        .class("space-y-4")
                        .children((
                            div()
                                .class("flex items-center justify-between")
                                .children((
                                    h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100")
                                        .children(i18n.t("provider_add")),
                                    button()
                                        .attr("type", "button")
                                        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                        .on(events::click, move |_| show_add_modal.set(false))
                                        .children(i().class("fas fa-times")),
                                )),
                            div().children((
                                label()
                                    .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                    .children(i18n.t("name")),
                                input()
                                    .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                                    .attr("type", "text")
                                    .attr("placeholder", i18n.t("name"))
                                    .bind(bind::value, form_name),
                            )),
                            div().children((
                                label()
                                    .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                    .children(i18n.t("provider_api_type")),
                                select()
                                    .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                                    .bind(bind::value, form_type)
                                    .children((
                                        option().attr("value", "open_ai_compat").children("OpenAI Compatible"),
                                        option().attr("value", "deep_seek").children("DeepSeek"),
                                        option().attr("value", "zhipu").children("Zhipu"),
                                        option().attr("value", "ollama").children("Ollama"),
                                        option().attr("value", "llama_cpp").children("Llama.cpp"),
                                    )),
                            )),
                            div().children((
                                label()
                                    .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                    .children(i18n.t("provider_base_url")),
                                input()
                                    .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                                    .attr("type", "text")
                                    .attr("placeholder", i18n.t("provider_base_url"))
                                    .bind(bind::value, form_base_url),
                            )),
                            div().children((
                                label()
                                    .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                    .children(i18n.t("api_key")),
                                input()
                                    .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                                    .attr("type", "text")
                                    .attr("placeholder", i18n.t("api_key"))
                                    .bind(bind::value, form_api_key),
                            )),
                            div().class("flex items-center gap-2").children((
                                input()
                                    .attr("type", "checkbox")
                                    .attr("id", "add-enabled")
                                    .attr("checked", "true")
                                    .on(events::click, move |_| form_enabled.set(!form_enabled.get())),
                                label()
                                    .attr("for", "add-enabled")
                                    .class("text-sm text-gray-700 dark:text-gray-300")
                                    .children(i18n.t("status_enabled")),
                            )),
                            View::from_dynamic(move || {
                                let err = form_error.get_clone();
                                if err.is_empty() {
                                    View::new()
                                } else {
                                    p().class("text-red-500 text-sm").children(err).into()
                                }
                            }),
                            div().class("flex items-center justify-end gap-3").children((
                                button()
                                    .attr("type", "button")
                                    .class("px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 cursor-pointer transition-colors")
                                    .on(events::click, move |_| show_add_modal.set(false))
                                    .children(i18n.t("cancel")),
                                button()
                                    .attr("type", "submit")
                                    .disabled(move || form_loading.get())
                                    .class("px-4 py-2 bg-blue-500 hover:enabled:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2")
                                    .children(View::from_dynamic(move || -> View {
                                        if form_loading.get() {
                                            div().class("flex items-center gap-2").children((
                                                i().class("fas fa-spinner animate-spin"),
                                                span().children(i18n_save.t("save")),
                                            )).into()
                                        } else {
                                            span().children(i18n_save.t("save")).into()
                                        }
                                    })),
                            )),
                        )),
                ),
        ))
        .into()
}

fn make_provider_rows(
    providers: Vec<Provider>,
    i18n: &I18n,
    show_detail: Signal<Option<usize>>,
    is_admin: sycamore::reactive::Signal<bool>,
) -> Vec<View> {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    providers
        .into_iter()
        .enumerate()
        .map(|(idx, prov)| {
            let ec = if prov.enabled {
                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
            } else {
                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
            };
            let st = if prov.enabled {
                &enabled_text
            } else {
                &disabled_text
            };
            let bg = if idx % 2 == 0 {
                ""
            } else {
                "bg-gray-50 dark:bg-gray-800/50"
            };
            let name = prov.name;
            let ptype = prov.provider_type;
            let url = prov.base_url;
            let span_class = format!(
                "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
                ec
            );
            let show = show_detail;
            tr().class(bg)
                .children((
                    td().class("px-6 py-4 font-medium text-gray-800 dark:text-gray-200")
                        .children(name),
                    td().class("px-6 py-4 text-gray-600 dark:text-gray-400")
                        .children(ptype),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono")
                        .children(url),
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
                                        .on(events::click, move |_| {
                                            sycamore::web::console_log!("Edit provider {}", idx);
                                        })
                                        .children(i().class("fas fa-pen text-xs")),
                                    button()
                                        .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                        .on(events::click, move |_| {
                                            sycamore::web::console_log!("Delete provider {}", idx);
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
    pub is_admin: sycamore::reactive::Signal<bool>,
    pub provider_refresh: sycamore::reactive::Signal<usize>,
    pub provider_refreshing: sycamore::reactive::Signal<bool>,
}

#[component]
pub fn ProviderTable(props: ProviderTableProps) -> View {
    let i18n = use_context::<I18n>();
    let show_detail = create_signal::<Option<usize>>(None);
    let providers = props.providers;
    let is_admin = props.is_admin;
    let provider_refresh = props.provider_refresh;
    let provider_refreshing = props.provider_refreshing;
    let rows = make_provider_rows(providers.clone(), &i18n, show_detail, is_admin);

    let count = providers.len();
    let i18n_modal = i18n.clone();
    let i18n_add = i18n.clone();
    let modal = View::from_dynamic(move || match show_detail.get() {
        Some(idx) => providers.get(idx).map_or(View::new(), |prov| {
            render_detail_modal(&i18n_modal, prov.clone(), show_detail)
        }),
        None => View::new(),
    });

    let show_add_modal = create_signal(false);
    let add_modal = View::from_dynamic(move || {
        if show_add_modal.get() {
            render_add_modal(&i18n_add, provider_refresh, show_add_modal)
        } else {
            View::new()
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
                                    .on(events::click, move |_| {
                                        show_add_modal.set(true);
                                    })
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
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("name")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("provider_api_type")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("provider_base_url")),
                            th().class(
                                "text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("table_status")),
                            th().class(
                                "text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("provider_detail")),
                            th().class(
                                "text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium",
                            )
                            .children(i18n.t("provider_actions")),
                        )),
                    ),
                    tbody().children(rows),
                )),
            ),
            modal,
            add_modal,
        ))
        .into()
}
