use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::{create_api_key, delete_api_key, toggle_api_key};
use crate::components::data_table::{
    debounce_refresh, render_table_header, table_container, table_shell, th_center, th_left,
};
use crate::components::delete_confirm::render_delete_confirm;
use crate::components::modal::{
    action_cell, form_error, form_field, form_input, form_submit_footer, modal_dialog,
    modal_title, mono_cell, name_cell, status_badge, text_cell, timestamp_cell, zebra_bg,
};
use crate::i18n::I18n;
use crate::models::ApiKeyListItem;

fn render_create_modal(
    i18n: &I18n,
    username: String,
    api_key_refresh: Signal<usize>,
    show_create_modal: Signal<bool>,
) -> View {
    let form_name = create_signal(String::new());
    let form_expires_date = create_signal(String::new());
    let form_expires_time = create_signal(String::new());
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
                Some((ref key, ref name)) => {
                    let copied = create_signal(false);
                    let key_for_copy = key.clone();
                    div().class("space-y-4").children((
                        modal_title(i18n.t("api_key_created"), backdrop_close),
                        p().class("text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-2 rounded-lg")
                            .children(i18n.t("api_key_raw_key_hint")),
                        form_field(
                            i18n.t("api_key_name"),
                            span().class("text-gray-900 dark:text-gray-100 font-medium").children(name.clone()).into(),
                        ),
                        form_field(
                            i18n.t("api_key_key"),
                            div().class("flex items-center gap-2").children((
                                div().class("flex-1 bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded-lg text-sm font-mono break-all text-gray-800 dark:text-gray-200 select-all").children(key.clone()),
                                button()
                                    .attr("type", "button")
                                    .class("shrink-0 px-3 py-2 text-sm text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-gray-100 border border-gray-300 dark:border-gray-600 rounded-lg transition-colors cursor-pointer")
                                    .on(events::click, {
                                        let k = key_for_copy.clone();
                                        move |_| {
                                            if let Some(w) = web_sys::window() {
                                                let _ = w.navigator().clipboard().write_text(&k);
                                                copied.set(true);
                                            }
                                        }
                                    })
                                    .children(i().class("fas fa-copy")),
                            )).into(),
                        ),
                        View::from_dynamic({
                            let i18n = i18n.clone();
                            move || {
                                if copied.get() {
                                    p().class("text-sm text-green-600 dark:text-green-400").children(i18n.t("copied_success")).into()
                                } else {
                                    View::new()
                                }
                            }
                        }),
                        div().class("flex justify-end").children(
                            button()
                                .attr("type", "button")
                                .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors cursor-pointer")
                                .on(events::click, backdrop_close)
                                .children(i18n.t("close")),
                        ),
                    )).into()
                },
                None => form()
                    .on(events::submit, {
                        let uname = uname.clone();
                        move |ev: web_sys::SubmitEvent| {
                            ev.prevent_default();
                            if form_loading.get() { return; }
                            let n = form_name.get_clone();
                            if n.is_empty() {
                                form_err.set("Name is required".into());
                                return;
                            }
                            form_loading.set(true);
                            form_err.set(String::new());
                            let expires_at = {
                                let date = form_expires_date.get_clone();
                                let time = form_expires_time.get_clone();
                                if date.is_empty() {
                                    None
                                } else {
                                    let dt_str = if time.is_empty() {
                                        format!("{}T00:00", date)
                                    } else {
                                        format!("{}T{}", date, time)
                                    };
                                    let ts = js_sys::Date::new(&dt_str.into()).get_time();
                                    if ts.is_nan() { None } else { Some((ts / 1000.0) as i64) }
                                }
                            };
                            let uname = uname.clone();
                            spawn_local_scoped(async move {
                                match create_api_key(&uname, &n, expires_at).await {
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
                            div().class("flex gap-2").children((
                                input()
                                    .class("flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:border-indigo-500 outline-none")
                                    .attr("type", "date")
                                    .bind(sycamore::web::bind::value, form_expires_date),
                                input()
                                    .class("w-28 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:border-indigo-500 outline-none")
                                    .attr("type", "time")
                                    .bind(sycamore::web::bind::value, form_expires_time),
                            )).into()),
                        form_error(form_err),
                        form_submit_footer(i18n.t("cancel"), backdrop_close, form_loading, i18n.t("save_create")),
                    ))
                    .into(),
            }
        }})),
        backdrop_close,
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
            let toggle_enabled = item.enabled;
            let toggle_key_id = item.id.clone();
            let uname = username.clone();
            tr().class(zebra_bg(idx))
                .children((
                    name_cell(item.name),
                    mono_cell(item.key),
                    timestamp_cell(item.created_at),
                    text_cell(match item.expires_at {
                        Some(ts) => crate::models::format_timestamp(ts),
                        None => "—".into(),
                    }),
                    text_cell(status_badge(item.enabled, &enabled_text, &disabled_text)),
                    timestamp_cell(item.updated_at),
                    action_cell(
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
                                            Err(e) => { sycamore::web::console_log!("Failed: {}", e); }
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

    let rows = make_api_key_rows(
        keys.clone(), &i18n, username.clone(), api_key_refresh, show_delete_confirm,
    );
    let count = keys.len();

    let header = render_table_header(
        &i18n, i18n.t("api_key_title"), count, api_key_refreshing,
        debounce_refresh(api_key_refresh, api_key_refreshing),
        {
            let i18n = i18n.clone();
            button()
                .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer")
                .on(events::click, {
                    let scm = show_create_modal;
                    move |_| scm.set(true)
                })
                .children((
                    i().class("fas fa-plus"),
                    span().children(i18n.t("api_key_create")),
                ))
                .into()
        },
    );

    let table = table_shell(
        vec![
            th_left(i18n.t("name")),
            th_left(i18n.t("api_key_key")),
            th_left(i18n.t("created_at")),
            th_left(i18n.t("expires_at")),
            th_left(i18n.t("table_status")),
            th_left(i18n.t("updated_at")),
            th_center(i18n.t("provider_actions")),
        ],
        rows,
    );

    let create_modal = View::from_dynamic({
        let i18n = i18n.clone();
        let uname = username.clone();
        move || if show_create_modal.get() {
            render_create_modal(&i18n, uname.clone(), api_key_refresh, show_create_modal)
        } else { View::new() }
    });

    let delete_modal = View::from_dynamic({
        let i18n = i18n.clone();
        let uname = username.clone();
        move || match show_delete_confirm.get_clone() {
            Some(item) => {
                let deleting = create_signal(false);
                let key_id = item.id.clone();
                let u = uname.clone();
                let refresh = api_key_refresh;
                render_delete_confirm(
                    &i18n,
                    i18n.t_replace("api_key_delete_confirm", "name", &item.name),
                    deleting,
                    move |_| {
                        if deleting.get() { return; }
                        deleting.set(true);
                        let kid = key_id.clone();
                        let u = u.clone();
                        let d = deleting;
                        spawn_local_scoped(async move {
                            match delete_api_key(&u, &kid).await {
                                Ok(_) => { refresh.update(|v| *v += 1); }
                                Err(e) => { d.set(false); sycamore::web::console_log!("Failed: {}", e); }
                            }
                        });
                    },
                    move |_| show_delete_confirm.set(None),
                )
            }
            None => View::new(),
        }
    });

    table_container(header, table, vec![create_modal, delete_modal])
}
