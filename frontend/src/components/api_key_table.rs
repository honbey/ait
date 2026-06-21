use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_api_key, delete_api_key, toggle_api_key};
use crate::components::modal::{
    form_delete_footer, form_error, form_field, form_input, form_submit_footer, modal_dialog,
    modal_title,
};
use crate::i18n::I18n;
use crate::models::{ApiKeyListItem, format_timestamp};

fn render_create_modal(
    i18n: &I18n,
    username: String,
    api_key_refresh: Signal<usize>,
    show_create_modal: Signal<bool>,
) -> View {
    let form_name = create_signal(String::new());
    let form_expires = create_signal(String::new());
    let form_err = create_signal(String::new());
    let form_loading = create_signal(false);
    let result = create_signal::<Option<(String, String)>>(None);

    let backdrop_close = move |_| {
        show_create_modal.set(false);
        api_key_refresh.update(|v| *v += 1);
    };

    modal_dialog(
        div().class("space-y-4").children(View::from_dynamic::<View>({
            let i18n = i18n.clone();
            let uname = username.clone();
            move || -> View { match result.get_clone() {
                Some((ref key, ref name)) => div().class("space-y-4").children((
                    modal_title(i18n.t("api_key_created"), backdrop_close),
                    p().class("text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-2 rounded-lg")
                        .children(i18n.t("api_key_raw_key_hint")),
                    form_field(
                        i18n.t("api_key_name"),
                        span().class("text-gray-900 dark:text-gray-100 font-medium").children(name.clone()).into(),
                    ),
                    form_field(
                        i18n.t("api_key_key"),
                        div().class("bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded-lg text-sm font-mono break-all text-gray-800 dark:text-gray-200 select-all").children(key.clone()).into(),
                    ),
                    div().class("flex justify-end").children(
                        button()
                            .attr("type", "button")
                            .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors cursor-pointer")
                            .on(events::click, backdrop_close)
                            .children(i18n.t("close")),
                    ),
                )).into(),
                None => form()
                    .on(events::submit, {
                        let uname = uname.clone();
                        move |ev: web_sys::SubmitEvent| {
                            ev.prevent_default();
                            if form_loading.get() {
                                return;
                            }
                            let n = form_name.get_clone();
                            if n.is_empty() {
                                form_err.set("Name is required".to_string());
                                return;
                            }
                            form_loading.set(true);
                            form_err.set(String::new());
                            let name = n;
                            let uname = uname.clone();
                            let expires_at = {
                                let raw = form_expires.get_clone();
                                if raw.is_empty() {
                                    None
                                } else {
                                    let ts = js_sys::Date::new(&raw.into()).get_time();
                                    if ts.is_nan() { None } else { Some((ts / 1000.0) as i64) }
                                }
                            };
                            spawn_local_scoped(async move {
                                match create_api_key(&uname, &name, expires_at).await {
                                    Ok(resp) => {
                                        form_loading.set(false);
                                        result.set(Some((resp.key, resp.name)));
                                    }
                                    Err(e) => {
                                        form_loading.set(false);
                                        form_err.set(e.to_string());
                                    }
                                }
                            });
                        }
                    })
                    .class("space-y-4")
                    .children((
                        modal_title(i18n.t("api_key_create"), backdrop_close),
                        form_field(i18n.t("api_key_name"), form_input(i18n.t("api_key_name"), form_name)),
                        form_field(i18n.t("expires_at"),
                            input()
                                .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                                .attr("type", "datetime-local")
                                .bind(sycamore::web::bind::value, form_expires)
                                .into()),
                        form_error(form_err),
                        form_submit_footer(
                            i18n.t("cancel"),
                            backdrop_close,
                            form_loading,
                            i18n.t("save_create"),
                        ),
                    ))
                    .into(),
            }
        }})),
        backdrop_close,
    )
}

fn render_delete_confirm(
    i18n: &I18n,
    username: String,
    api_key_refresh: Signal<usize>,
    show_delete_confirm: Signal<Option<ApiKeyListItem>>,
    item: ApiKeyListItem,
) -> View {
    let deleting = create_signal(false);
    let key_id = item.id.clone();
    let on_delete = move |_| {
        if deleting.get() {
            return;
        }
        deleting.set(true);
        let kid = key_id.clone();
        let uname = username.clone();
        let refresh = api_key_refresh;
        let d = deleting;
        spawn_local_scoped(async move {
            match delete_api_key(&uname, &kid).await {
                Ok(_) => {
                    refresh.update(|v| *v += 1);
                }
                Err(e) => {
                    d.set(false);
                    sycamore::web::console_log!("Failed to delete API key: {}", e);
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
                .children(i18n.t_replace("api_key_delete_confirm", "name", &item.name)),
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

fn make_api_key_rows(
    keys: Vec<ApiKeyListItem>,
    i18n: &I18n,
    username: String,
    api_key_refresh: Signal<usize>,
    show_delete_confirm: Signal<Option<ApiKeyListItem>>,
) -> Vec<View> {
    let enabled_text = i18n.t("status_enabled");
    let disabled_text = i18n.t("status_disabled");
    keys.into_iter()
        .enumerate()
        .map(|(idx, item)| {
            let item_modal = item.clone();
            let ec = if item.enabled {
                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
            } else {
                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
            };
            let st = if item.enabled { &enabled_text } else { &disabled_text };
            let bg = if idx % 2 == 0 { "" } else { "bg-gray-50 dark:bg-gray-800/50" };
            let span_class = format!("inline-block px-2 py-1 rounded-full text-xs font-medium {}", ec);
            let toggle_enabled = item.enabled;
            let toggle_key_id = item.id.clone();
            let uname = username.clone();
            tr().class(bg)
                .children((
                    td().class("px-6 py-4 font-medium text-gray-800 dark:text-gray-200")
                        .children(item.name),
                    td().class("px-6 py-4 font-mono text-xs text-gray-500 dark:text-gray-400 max-w-[200px] truncate")
                        .children(item.key),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
                        .children(format_timestamp(item.created_at)),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
                        .children(match item.expires_at {
                            Some(timestamp) => format_timestamp(timestamp),
                            None => "—".to_string(),
                        }),
                    td().class("px-6 py-4")
                        .children(span().class(span_class).children(st.clone())),
                    td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
                        .children(format_timestamp(item.updated_at)),
                    td().class("px-6 py-4 text-center whitespace-nowrap").children(
                        div().class("flex items-center justify-center gap-3").children((
                            button()
                                .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                .on(events::click, move |_| {
                                    let kid = toggle_key_id.clone();
                                    let u = uname.clone();
                                    let refresh = api_key_refresh;
                                    spawn_local_scoped(async move {
                                        match toggle_api_key(&u, &kid, !toggle_enabled).await {
                                            Ok(_) => { refresh.update(|v| *v += 1); }
                                            Err(e) => { sycamore::web::console_log!("Failed to toggle API key: {}", e); }
                                        }
                                    });
                                })
                                .children(i().class(move || {
                                    if toggle_enabled { "fas fa-toggle-on text-xs" } else { "fas fa-toggle-off text-xs" }
                                })),
                            button()
                                .class("cursor-pointer text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors")
                                .on(events::click, {
                                    let p = item_modal.clone();
                                    move |_| show_delete_confirm.set(Some(p.clone()))
                                })
                                .children(i().class("fas fa-trash text-xs")),
                        )),
                    ),
                ))
                .into()
        })
        .collect()
}

#[derive(Props)]
pub struct ApiKeyTableProps {
    pub keys: Vec<ApiKeyListItem>,
    pub username: String,
    pub api_key_refresh: Signal<usize>,
    pub api_key_refreshing: Signal<bool>,
}

#[component]
pub fn ApiKeyTable(props: ApiKeyTableProps) -> View {
    let i18n = use_context::<I18n>();
    let show_create_modal = create_signal(false);
    let show_delete_confirm = create_signal::<Option<ApiKeyListItem>>(None);
    let keys = props.keys;
    let username = props.username;
    let api_key_refresh = props.api_key_refresh;
    let api_key_refreshing = props.api_key_refreshing;
    let rows = make_api_key_rows(keys.clone(), &i18n, username.clone(), api_key_refresh, show_delete_confirm);
    let count = keys.len();

    let create_modal = View::from_dynamic({
        let i18n = i18n.clone();
        let uname = username.clone();
        move || {
            if show_create_modal.get() {
                render_create_modal(&i18n, uname.clone(), api_key_refresh, show_create_modal)
            } else {
                View::new()
            }
        }
    });

    let delete_confirm = View::from_dynamic({
        let i18n = i18n.clone();
        let uname = username.clone();
        move || match show_delete_confirm.get_clone() {
            Some(item) => render_delete_confirm(
                &i18n,
                uname.clone(),
                api_key_refresh,
                show_delete_confirm,
                item,
            ),
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
                            .children(i18n.t("api_key_title")),
                        span().class(
                            "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                        )
                        .children(i18n.t_replace("total_count", "count", &count.to_string())),
                        button()
                            .disabled(move || api_key_refreshing.get())
                            .class("text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed")
                            .on(events::click, move |_| {
                                if api_key_refreshing.get() { return; }
                                api_key_refreshing.set(true);
                                let r = api_key_refresh;
                                Timeout::new(50, move || { r.update(|v| *v += 1); }).forget();
                            })
                            .children(i().class(move || {
                                if api_key_refreshing.get() { "fas fa-sync-alt animate-spin" } else { "fas fa-sync-alt" }
                            })),
                    )),
                    button()
                        .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer")
                        .on(events::click, move |_| show_create_modal.set(true))
                        .children((
                            i().class("fas fa-plus"),
                            span().children(i18n.t("api_key_create")),
                        )),
                )),
            div().class("overflow-x-auto").children(
                table().class("w-full text-sm").children((
                    thead().children(
                        tr().class("border-b border-gray-100 dark:border-gray-700")
                            .children((
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("name")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("api_key_key")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("created_at")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("expires_at")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("table_status")),
                                th().class("text-left px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("updated_at")),
                                th().class("text-center px-6 py-3 text-gray-500 dark:text-gray-400 font-medium")
                                    .children(i18n.t("provider_actions")),
                            )),
                    ),
                    tbody().children(rows),
                )),
            ),
            create_modal,
            delete_confirm,
        ))
        .into()
}
