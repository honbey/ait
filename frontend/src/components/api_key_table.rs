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
    action_cell, form_checkbox, form_error, form_field, form_input, form_submit_footer,
    modal_dialog, modal_title, mono_cell, name_cell, status_badge, text_cell, timestamp_cell,
    zebra_bg,
};
use crate::i18n::{I18n, K};
use crate::models::ApiKeyListItem;

fn render_create_modal(
    username: String,
    api_key_refresh: Signal<usize>,
    show_create_modal: Signal<bool>,
) -> View {
    let i18n = use_context::<I18n>();
    let form_name = create_signal(String::new());
    let form_never_expire = create_signal(false);
    let form_expires = create_signal({
        let d = js_sys::Date::new_0();
        d.set_date(d.get_date() + 30);
        format!(
            "{:04}-{:02}-{:02}T00:00",
            d.get_full_year(),
            d.get_month() + 1,
            d.get_date(),
        )
    });
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
                        modal_title(i18n.t(K::ApiKeyCreated), backdrop_close),
                        p().class("text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-2 rounded-lg")
                            .children(i18n.t(K::ApiKeyRawKeyHint)),
                        div().children((
                            label().class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                .children(i18n.t(K::ApiKeyName)),
                            span().class("text-gray-900 dark:text-gray-100 font-medium").children(name.clone()),
                        )),
                        div().children((
                            label().class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                                .children(i18n.t(K::ApiKeyKey)),
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
                            )),
                        )),
                        View::from_dynamic({
                            let i18n = i18n.clone();
                            move || {
                                if copied.get() {
                                    p().class("text-sm text-green-600 dark:text-green-400").children(i18n.t(K::CopiedSuccess)).into()
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
                                .children(i18n.t(K::Close)),
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
                            let expires_at = if form_never_expire.get() {
                                None
                            } else {
                                let dt = form_expires.get_clone();
                                if dt.is_empty() { None } else {
                                    let ts = js_sys::Date::new(&dt.into()).get_time();
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
                        modal_title(i18n.t(K::ApiKeyCreate), backdrop_close),
                        form_field("create-apikey-name".into(), i18n.t(K::ApiKeyName), form_input("create-apikey-name".into(), i18n.t(K::ApiKeyName), form_name)),
                        form_field("create-apikey-expires".into(), i18n.t(K::ExpiresAt),
                            input()
                                .attr("id", "create-apikey-expires")
                                .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:border-indigo-500 outline-none")
                                .attr("type", "datetime-local")
                                .bool_attr("disabled", move || form_never_expire.get())
                                .bind(sycamore::web::bind::value, form_expires)
                                .into()),
                        form_checkbox("create-apikey-never-expire".into(), i18n.t(K::NeverExpires), form_never_expire),
                        form_error(form_err),
                        form_submit_footer(i18n.t(K::Cancel), backdrop_close, form_loading, i18n.t(K::SaveCreate)),
                    ))
                    .into(),
            }
        }})),
        backdrop_close,
    )
}

fn make_api_key_rows(
    keys: Vec<ApiKeyListItem>,
    username: String,
    api_key_refresh: Signal<usize>,
    show_delete_confirm: Signal<Option<ApiKeyListItem>>,
) -> Vec<View> {
    let i18n = use_context::<I18n>();
    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);
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
                                .on(events::click,                                 move |_| {
                                    let kid = toggle_key_id.clone();
                                    let u = uname.clone();
                                    spawn_local_scoped(async move {
                                        match toggle_api_key(&u, &kid, !toggle_enabled).await {
                                            Ok(_) => { api_key_refresh.update(|v| *v += 1); }
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
        keys.clone(),
        username.clone(),
        api_key_refresh,
        show_delete_confirm,
    );
    let count = keys.len();

    let header = render_table_header(
        i18n.t(K::ApiKeyTitle),
        count,
        api_key_refreshing,
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
                    span().children(i18n.t(K::ApiKeyCreate)),
                ))
                .into()
        },
    );

    let table = table_shell(
        vec![
            th_left(i18n.t(K::Name)),
            th_left(i18n.t(K::ApiKeyKey)),
            th_left(i18n.t(K::CreatedAt)),
            th_left(i18n.t(K::ExpiresAt)),
            th_left(i18n.t(K::TableStatus)),
            th_left(i18n.t(K::UpdatedAt)),
            th_center(i18n.t(K::Actions)),
        ],
        rows,
    );

    let create_modal = View::from_dynamic({
        let uname = username.clone();
        move || {
            if show_create_modal.get() {
                render_create_modal(uname.clone(), api_key_refresh, show_create_modal)
            } else {
                View::new()
            }
        }
    });

    let delete_modal = View::from_dynamic({
        let i18n = i18n.clone();
        let uname = username.clone();
        move || match show_delete_confirm.get_clone() {
            Some(item) => {
                let deleting = create_signal(false);
                let key_id = item.id.clone();
                let u = uname.clone();
                render_delete_confirm(
                    i18n.t_replace(K::DeleteConfirmMessage, "name", &item.name),
                    deleting,
                    move |_| {
                        if deleting.get() {
                            return;
                        }
                        deleting.set(true);
                        let kid = key_id.clone();
                        let u = u.clone();
                        spawn_local_scoped(async move {
                            match delete_api_key(&u, &kid).await {
                                Ok(_) => {
                                    api_key_refresh.update(|v| *v += 1);
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

    table_container(header, table, vec![create_modal, delete_modal])
}
