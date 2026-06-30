use std::rc::Rc;

use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;
use wasm_bindgen_futures::JsFuture;

use crate::api::{create_api_key, delete_api_key, toggle_api_key};
use crate::components::delete_confirm::render_delete_confirm;
use crate::components::modal::{
    CLASS_LABEL, FormStatus, action_cell, blue_add_button, form_checkbox, form_error, form_field,
    form_input, form_submit_footer, icon_button, modal_dialog, modal_title, mono_cell, name_cell,
    status_badge, text_cell, timestamp_cell,
};
use crate::components::table::{
    Column, CrudModal, common_table, debounce_refresh, th_center, th_left,
};
use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};
use crate::models::ApiKeyListItem;

struct ApiKeyForm {
    name: Signal<String>,
    never_expire: Signal<bool>,
    expires: Signal<String>,
}

impl ApiKeyForm {
    fn new() -> Self {
        Self {
            name: create_signal(String::new()),
            never_expire: create_signal(false),
            expires: create_signal({
                let d = js_sys::Date::new_0();
                d.set_date(d.get_date() + 30);
                format!(
                    "{:04}-{:02}-{:02}T00:00",
                    d.get_full_year(),
                    d.get_month() + 1,
                    d.get_date(),
                )
            }),
        }
    }
}

fn render_create_modal(
    username: String,
    api_key_refresh: Signal<usize>,
    modal: Signal<CrudModal<ApiKeyListItem>>,
) -> View {
    let i18n = use_context::<I18n>();
    let ApiKeyForm {
        name: form_name,
        never_expire: form_never_expire,
        expires: form_expires,
    } = ApiKeyForm::new();
    let FormStatus {
        err: form_err,
        loading: form_loading,
    } = FormStatus::new();
    let result = create_signal::<Option<(String, String)>>(None);

    let backdrop_close = move |_| {
        modal.set(CrudModal::Closed);
        api_key_refresh.update(|v| *v += 1);
    };

    let copied = create_signal(false);
    modal_dialog(
        div().class("space-y-4").children(View::from_dynamic::<View>({
            let uname = username.clone();
            move || -> View { match result.get_clone() {
                Some((key, name)) => {
                    div().class("space-y-4").children((
                        modal_title(i18n.t(K::ApiKeyCreated), backdrop_close),
                        p().class("text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-2 rounded-lg")
                            .children(i18n.t(K::ApiKeyRawKeyHint)),
                        div().children((
                            label().class(CLASS_LABEL)
                                .children(i18n.t(K::ApiKeyName)),
                            span().class("text-gray-900 dark:text-gray-100 font-medium").children(name.clone()),
                        )),
                        div().children((
                            label().class(CLASS_LABEL)
                                .children(i18n.t(K::ApiKeyKey)),
                            div().class("flex items-center gap-2").children((
                                div().class("flex-1 bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded-lg text-sm font-mono break-all text-gray-800 dark:text-gray-200 select-all").children(key.clone()),
                                button()
                                    .attr("type", "button")
                                    .class("shrink-0 px-3 py-2 text-sm text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-gray-100 border border-gray-300 dark:border-gray-600 rounded-lg transition-colors cursor-pointer")
                                    .on(events::click, {
                                        let k = key.clone();
                                        move |_| {
                                            if let Some(w) = web_sys::window() {
                                                let promise = w.navigator().clipboard().write_text(&k);
                                                spawn_local_scoped(async move {
                                                    if JsFuture::from(promise).await.is_ok() {
                                                        copied.set(true);
                                                    }
                                                });
                                            }
                                        }
                                    })
                                    .children(i().class("fas fa-copy")),
                            )),
                        )),
                        if copied.get() {
                            p().class("text-sm text-green-600 dark:text-green-400").children(i18n.t(K::CopiedSuccess)).into()
                        } else {
                            View::new()
                        },
                        div().class("flex justify-end").children(
                            button()
                                .attr("type", "button")
                                .class("px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors cursor-pointer")
                                .on(events::click, backdrop_close)
                                .children(i18n.t(K::Close)),
                        ),
                    )).into()
                },
                None => {
                    copied.set(false);
                    form()
                        .on(events::submit, {
                            let uname = uname.clone();
                            let i18n = i18n.clone();
                            move |ev: web_sys::SubmitEvent| {
                                ev.prevent_default();
                                if form_loading.get() { return; }
                                let n = form_name.get_clone();
                                if n.is_empty() {
                                    form_err.set(i18n.t(K::NameRequired));
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
                        .into()
                },
            }
        }})),
        backdrop_close,
    )
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
    let toast = use_context::<ToastManager>();
    let modal = create_signal::<CrudModal<ApiKeyListItem>>(CrudModal::Closed);
    let deleting = create_signal(false);
    let keys = props.keys;
    let username = props.username;
    let api_key_refresh = props.api_key_refresh;
    let api_key_refreshing = props.api_key_refreshing;

    let enabled_text = i18n.t(K::StatusEnabled);
    let disabled_text = i18n.t(K::StatusDisabled);

    let i18n_col = i18n.clone();
    let username_col = username.clone();
    let toast_col = toast.clone();

    let columns = vec![
        Column {
            header: th_left(i18n.t(K::Name)),
            cell: Rc::new(|item: ApiKeyListItem| name_cell(item.name)),
        },
        Column {
            header: th_left(i18n.t(K::ApiKeyKey)),
            cell: Rc::new(|item: ApiKeyListItem| mono_cell(item.key)),
        },
        Column {
            header: th_left(i18n.t(K::CreatedAt)),
            cell: Rc::new(|item: ApiKeyListItem| timestamp_cell(item.created_at)),
        },
        Column {
            header: th_left(i18n.t(K::ExpiresAt)),
            cell: Rc::new(|item: ApiKeyListItem| {
                td().class("px-6 py-4 text-gray-400 dark:text-gray-500 text-sm")
                    .children(match item.expires_at {
                        Some(ts) => crate::models::format_timestamp(ts),
                        None => "—".into(),
                    })
                    .into()
            }),
        },
        Column {
            header: th_left(i18n.t(K::TableStatus)),
            cell: {
                let et = enabled_text.clone();
                let dt = disabled_text.clone();
                Rc::new(move |item: ApiKeyListItem| text_cell(status_badge(item.enabled, &et, &dt)))
            },
        },
        Column {
            header: th_left(i18n.t(K::UpdatedAt)),
            cell: Rc::new(|item: ApiKeyListItem| timestamp_cell(item.updated_at)),
        },
        Column {
            header: th_center(i18n.t(K::Actions)),
            cell: Rc::new(move |item: ApiKeyListItem| {
                let toggle_kid = item.id.clone();
                let toggle_enabled = item.enabled;
                let toggle_icon: &str = if toggle_enabled {
                    "fas fa-toggle-on"
                } else {
                    "fas fa-toggle-off"
                };
                let delete_item = item.clone();
                let uname = username_col.clone();
                let toast = toast_col.clone();
                let i18n = i18n_col.clone();
                action_cell(
                    div()
                        .class("flex items-center justify-center gap-3")
                        .children((
                            icon_button(toggle_icon, move |_| {
                                let kid = toggle_kid.clone();
                                let u = uname.clone();
                                let toast = toast.clone();
                                let i18n = i18n.clone();
                                let refresh = api_key_refresh;
                                spawn_local_scoped(async move {
                                    match toggle_api_key(&u, &kid, !toggle_enabled).await {
                                        Ok(_) => {
                                            toast.success(&if toggle_enabled {
                                                i18n.t(K::ApiKeyDisabled)
                                            } else {
                                                i18n.t(K::ApiKeyEnabled)
                                            });
                                            refresh.update(|v| *v += 1);
                                        }
                                        Err(e) => {
                                            toast.error(e.to_string());
                                        }
                                    }
                                });
                            }),
                            icon_button("fas fa-trash", move |_| {
                                modal.set(CrudModal::Delete(delete_item.clone()))
                            }),
                        )),
                )
            }),
        },
    ];

    let add_button = blue_add_button(i18n.t(K::ApiKeyCreate), move |_| modal.set(CrudModal::Add));

    let username_m = username.clone();
    let i18n_m = i18n.clone();
    let toast_m = toast.clone();
    let modals = View::from_dynamic(move || match modal.get_clone() {
        CrudModal::Add => render_create_modal(username.clone(), api_key_refresh, modal),
        CrudModal::Delete(item) => render_delete_confirm(
            i18n_m.t_replace(K::DeleteConfirmMessage, "name", &item.name),
            deleting,
            {
                let u = username_m.clone();
                let toast = toast_m.clone();
                let i18n = i18n_m.clone();
                move |_| {
                    if deleting.get() {
                        return;
                    }
                    deleting.set(true);
                    let kid = item.id.clone();
                    let u = u.clone();
                    let toast = toast.clone();
                    let i18n = i18n.clone();
                    spawn_local_scoped(async move {
                        match delete_api_key(&u, &kid).await {
                            Ok(_) => {
                                toast.success(i18n.t(K::ApiKeyDeleted));
                                api_key_refresh.update(|v| *v += 1);
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
        CrudModal::Detail(_) | CrudModal::Edit(_) | CrudModal::Closed => {
            deleting.set(false);
            View::new()
        }
    });

    common_table(
        i18n.t(K::ApiKeyTitle),
        keys,
        api_key_refreshing,
        debounce_refresh(api_key_refresh, api_key_refreshing),
        columns,
        add_button,
        vec![modals],
    )
}
