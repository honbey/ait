use leptos::prelude::*;
use reactive_graph::traits::{Get, Set, Write};
use reactive_stores::{Field, Patch, Store};

use crate::api;
use crate::api::{ApiKey, ApiKeyStoreFields};
use crate::auth::AuthContext;

use crate::components::error_display::{ErrorCard, ErrorText};
use crate::components::modal::{DeleteConfirmModal, FormModalShell, ModalShell};
use crate::components::skeleton::table_skeleton;
use crate::components::style::{
    CLASS_BORDER_B, CLASS_BTN_PRIMARY, CLASS_DETAIL_DIVIDER, CLASS_DETAIL_VALUE_MONO,
    CLASS_DETAIL_VALUE_PLAIN, CLASS_DETAIL_VALUE_TAG, CLASS_ICON_BTN, CLASS_INPUT, CLASS_LABEL,
    CLASS_PAGE_TITLE, CLASS_TEXT_MUTED,
};
use crate::components::table::{
    DataTableCard, DetailCloseButton, DetailRow, EntityModal, ToggleField, status_badge,
    timestamp_str,
};
use crate::components::toast::use_toast;
use crate::components::use_page_title;
use crate::time_utils::{date_str_to_ts, ts_to_datetime_str};
use crate::{t, tr, trs, ts};

type ApiKeyModal = EntityModal<ApiKey>;

#[derive(Store, Patch, Default)]
struct ApiKeysStore {
    #[store(key: String = |k: &ApiKey| k.id.clone())]
    items: Vec<ApiKey>,
    error: Option<String>,
}

fn default_expiry_str() -> String {
    let d = js_sys::Date::new_0();
    d.set_date(d.get_date() + 30);
    let y = d.get_full_year();
    let mo = d.get_month() + 1;
    let day = d.get_date();
    format!("{:04}-{:02}-{:02}T00:00", y, mo, day)
}

fn expires_at_display(expires_at: Option<i64>) -> String {
    match expires_at {
        Some(ts) if ts > 0 => timestamp_str(ts),
        _ => "-".to_string(),
    }
}

#[component]
pub fn ApiKeysPage() -> impl IntoView {
    use_page_title(&format!("Ait - {}", ts!(ApiKey)));
    let modal = RwSignal::new(ApiKeyModal::Closed);
    let state = Store::new(ApiKeysStore::default());
    let auth = use_context::<AuthContext>().expect("AuthContext not provided");
    let created_raw_key = RwSignal::new(Option::<(String, String)>::None);

    let api_keys_rsc = LocalResource::new({
        let auth = auth.clone();
        move || {
            let name = auth.username.get_untracked().unwrap_or_default();
            async move {
                if name.is_empty() {
                    return Err("no username".to_string());
                }
                api::fetch_api_keys(&name).await.map_err(|e| e.to_string())
            }
        }
    });

    let _sync_store = Effect::new(move |_| match api_keys_rsc.get() {
        Some(Ok(ref items)) => {
            let items_field = state.items();
            let mut guard = items_field.write();
            guard.clone_from(items);
            drop(guard);
            state.error().set(None);
        }
        Some(Err(ref e)) => state.error().set(Some(e.to_string())),
        None => {}
    });

    let on_close = move || modal.set(ApiKeyModal::Closed);

    let do_refetch = move || api_keys_rsc.refetch();

    view! {
        <h1 class=CLASS_PAGE_TITLE>{tr!(ListTitle, &[("entity", &t!(ApiKey)())])}</h1>
        <DataTableCard
            item_count=Signal::derive(move || state.items().get().len())
            on_refresh=do_refetch
            on_add=move || modal.set(ApiKeyModal::Add)
            add_label=trs!(Add, &[("entity", &ts!(ApiKey))])
        >
            <Transition fallback=move || table_skeleton()>
                {move || match api_keys_rsc.get() {
                    Some(Ok(_)) => {
                        view! {
                            <div class="overflow-x-auto">
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr class=CLASS_BORDER_B>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(Name)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(ApiKey)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(ExpiresAt)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(TableStatus)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(UpdatedAt)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-center {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(Actions)}</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <For
                                            each=move || state.items()
                                            key=|row| row.clone().id().get()
                                            let:key
                                        >
                                            <tr class="odd:bg-gray-50 dark:odd:bg-gray-800/50">
                                                {
                                                    let k: Field<ApiKey> = key.into();
                                                    view! {
                                                        <td class="px-6 py-4 font-medium text-gray-800 dark:text-gray-200">
                                                            {move || k.name().get()}
                                                        </td>
                                                        <td class="px-6 py-4">
                                                            <span class=format!(
                                                                "font-mono text-xs {}",
                                                                CLASS_TEXT_MUTED,
                                                            )>{k.key().get()}</span>
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-sm">
                                                            {move || expires_at_display(k.expires_at().get())}
                                                        </td>
                                                        <td class="px-6 py-4">
                                                            {move || status_badge(k.enabled().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-sm">
                                                            {move || timestamp_str(k.updated_at().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-center whitespace-nowrap">
                                                            <div class="flex items-center justify-center gap-3">
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ApiKeyModal::Detail(k))
                                                                >
                                                                    <i class="fas fa-eye text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ApiKeyModal::Edit(k))
                                                                >
                                                                    <i class="fas fa-pen text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ApiKeyModal::Delete(k))
                                                                >
                                                                    <i class="fas fa-trash text-xs"></i>
                                                                </button>
                                                            </div>
                                                        </td>
                                                    }
                                                }
                                            </tr>
                                        </For>
                                    </tbody>
                                </table>
                            </div>
                        }
                            .into_any()
                    }
                    Some(Err(ref e)) => {
                        view! { <ErrorCard message=e.clone() on_retry=Box::new(do_refetch) /> }
                            .into_any()
                    }
                    None => ().into_any(),
                }}
            </Transition>
        </DataTableCard>

        {move || match (modal.get(), created_raw_key.get()) {
            (_, Some((raw, raw_name))) => {
                let copy_error = RwSignal::new(String::new());
                let copy_action: Action<String, Result<(), ()>> = Action::new_local(move |
                    input: &String|
                {
                    let input = input.clone();
                    async move {
                        match (|| -> Option<web_sys::Clipboard> {
                            Some(web_sys::window()?.navigator().clipboard())
                        })() {
                            Some(cb) => {
                                let promise = cb.write_text(&input);
                                match wasm_bindgen_futures::JsFuture::from(promise).await {
                                    Ok(_) => Ok(()),
                                    Err(e) => {
                                        leptos::logging::error!("Clipboard write failed: {:?}", e);
                                        copy_error.set(ts!(ClipboardCopyFailed));
                                        Err(())
                                    }
                                }
                            }
                            None => {
                                leptos::logging::error!("Clipboard not available");
                                copy_error.set(ts!(ClipboardCopyFailed));
                                Err(())
                            }
                        }
                    }
                });
                view! {
                    <ModalShell
                        on_close=move || {
                            created_raw_key.set(None);
                            on_close();
                        }
                        title=ts!(ApiKeyCreate)
                    >
                        <div class="space-y-4">
                            <p class="text-sm text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-2 rounded-lg">
                                {t!(ApiKeyRawKeyHint)}
                            </p>
                            <div>
                                <label class=format!(
                                    "text-sm {}",
                                    CLASS_TEXT_MUTED,
                                )>{t!(ApiKeyName)}</label>
                                <p class="text-gray-900 dark:text-gray-100 font-medium mt-0.5">
                                    {raw_name.clone()}
                                </p>
                            </div>
                            <div>
                                <label class=format!(
                                    "text-sm {}",
                                    CLASS_TEXT_MUTED,
                                )>{t!(ApiKeyKey)}</label>
                                <div class="flex items-center gap-2 mt-0.5">
                                    <div class="flex-1 bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded-lg text-sm font-mono break-all text-gray-800 dark:text-gray-200 select-all">
                                        {raw.clone()}
                                    </div>
                                    <button
                                        class="shrink-0 px-3 py-2 text-sm text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-gray-100 border border-gray-300 dark:border-gray-600 rounded-lg transition-colors cursor-pointer active:scale-95"
                                        on:click=move |_| {
                                            copy_action.dispatch(raw.clone());
                                        }
                                    >
                                        <i class="fas fa-copy"></i>
                                    </button>
                                </div>
                            </div>
                            {move || match copy_action.value().get() {
                                Some(Ok(())) => {
                                    view! {
                                        <p class="text-sm text-green-600 dark:text-green-400">
                                            {t!(CopiedSuccess)}
                                        </p>
                                    }
                                        .into_any()
                                }
                                Some(Err(())) => view! { <ErrorText msg=copy_error /> }.into_any(),
                                None => ().into_any(),
                            }}
                            <div class="flex justify-end">
                                <button
                                    type="button"
                                    class=CLASS_BTN_PRIMARY
                                    on:click=move |_| {
                                        created_raw_key.set(None);
                                        on_close();
                                    }
                                >
                                    {t!(Close)}
                                </button>
                            </div>
                        </div>
                    </ModalShell>
                }
                    .into_any()
            }
            (ApiKeyModal::Add, None) => {
                view! {
                    <ApiKeyFormModal
                        username=auth.username.get_untracked().unwrap_or_default()
                        state
                        created_raw_key
                        on_close
                    />
                }
                    .into_any()
            }
            (ApiKeyModal::Edit(key), None) => {
                view! {
                    <ApiKeyFormModal
                        username=auth.username.get_untracked().unwrap_or_default()
                        state
                        created_raw_key
                        on_close
                        edit_model=key
                    />
                }
                    .into_any()
            }
            (ApiKeyModal::Delete(key), None) => {
                let key_id = key.id().get();
                let item_name = key.name().get();
                let uname = auth.username.get_untracked().unwrap_or_default();
                let delete_action: Action<(), Result<(), String>> = Action::new_local({
                    let uname = uname.clone();
                    let kid = key_id.clone();
                    move |_: &()| {
                        let uname = uname.clone();
                        let kid = kid.clone();
                        async move {
                            api::delete_api_key(&uname, &kid).await.map_err(|e| e.to_string())
                        }
                    }
                });
                let on_success = move || {
                    state.items().write().retain(|k| k.id != key_id);
                    on_close();
                };
                view! {
                    <DeleteConfirmModal
                        entity_name=Box::new(t!(ApiKey))
                        item_name
                        action=delete_action
                        on_close
                        on_success
                    />
                }
                    .into_any()
            }
            (ApiKeyModal::Detail(key), None) => {
                view! { <ApiKeyDetailModal key=key on_close /> }.into_any()
            }
            (ApiKeyModal::Closed, None) => ().into_any(),
        }}
    }
}

#[derive(Clone, Debug)]
struct ApiKeyFormInput {
    name: String,
    expires_at: String,
    clear_expiry: bool,
    enabled: bool,
}

#[component]
fn ApiKeyFormModal(
    username: String,
    state: Store<ApiKeysStore>,
    created_raw_key: RwSignal<Option<(String, String)>>,
    on_close: impl Fn() + 'static + Clone + Send,
    #[prop(optional)] edit_model: Option<Field<ApiKey>>,
) -> impl IntoView {
    let is_edit = edit_model.is_some();
    let edit_id = edit_model.map(|f| f.id().get());
    let toast = use_toast();

    let name = RwSignal::new(edit_model.map(|f| f.name().get()).unwrap_or_default());
    let enabled = RwSignal::new(edit_model.map(|f| f.enabled().get()).unwrap_or(true));
    let expires_date = RwSignal::new(
        edit_model
            .and_then(|f| f.expires_at().get())
            .map(ts_to_datetime_str)
            .unwrap_or_else(default_expiry_str),
    );
    let clear_expiry = RwSignal::new(false);
    let form_error = RwSignal::new(String::new());

    let save_action: Action<ApiKeyFormInput, Result<ApiKey, String>> = Action::new_local({
        let username = username.clone();
        let edit_id = edit_id.clone();
        move |input: &ApiKeyFormInput| {
            let data = input.clone();
            let uname = username.clone();
            let edit_id = edit_id.clone();
            async move {
                let expires_ts = if data.clear_expiry {
                    Some(0)
                } else if data.expires_at.is_empty() {
                    None
                } else {
                    date_str_to_ts(&data.expires_at)
                };
                if let Some(id) = &edit_id {
                    api::update_api_key(
                        &uname,
                        id,
                        Some(&data.name),
                        expires_ts,
                        Some(data.enabled),
                    )
                    .await
                    .map_err(|e| e.to_string())
                } else {
                    api::create_api_key(&uname, &data.name, expires_ts)
                        .await
                        .map_err(|e| e.to_string())
                }
            }
        }
    });

    let on_submitted = on_close.clone();

    Effect::new(move |_| {
        if let Some(Ok(api_key)) = save_action.value().get() {
            if let Some(field) = edit_model {
                field.patch(api_key);
            } else {
                created_raw_key.set(Some((api_key.key.clone(), api_key.name.clone())));
                let mut masked = api_key;
                let k = masked.key.clone();
                masked.key = format!("{}...{}", &k[..6], &k[k.len() - 3..]);
                state.items().write().push(masked);
            }
            let action_label = if is_edit {
                ts!(ActionUpdated)
            } else {
                ts!(ActionCreated)
            };
            toast.success(trs!(
                EntityAction,
                &[("entity", &ts!(ApiKey)), ("action", &action_label)]
            ));
            on_submitted();
        }
    });

    Effect::new(move |_| {
        if let Some(Err(e)) = save_action.value().get() {
            form_error.set(e);
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if save_action.pending().get_untracked() {
            return;
        }
        let key_name = name.get_untracked();
        if key_name.is_empty() {
            form_error.set(ts!(NameRequired));
            return;
        }
        form_error.set(String::new());
        save_action.dispatch(ApiKeyFormInput {
            name: key_name,
            expires_at: expires_date.get_untracked(),
            clear_expiry: clear_expiry.get_untracked(),
            enabled: enabled.get_untracked(),
        });
    };

    let pending = save_action.pending();

    let title = if is_edit {
        trs!(Edit, &[("entity", &ts!(ApiKey))])
    } else {
        ts!(ApiKeyCreate)
    };

    view! {
        <FormModalShell on_close=on_close.clone() title=title on_submit pending is_edit form_error>
            <div>
                <label for="form-name" class=CLASS_LABEL>
                    {t!(ApiKeyName)}
                </label>
                <input
                    id="form-name"
                    type="text"
                    class=CLASS_INPUT
                    placeholder=ts!(ApiKeyName)
                    prop:value=name
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
            </div>

            <div>
                <label for="form-expires" class=CLASS_LABEL>
                    {t!(ExpiresAt)}
                </label>
                <input
                    id="form-expires"
                    type="datetime-local"
                    class=CLASS_INPUT
                    prop:value=expires_date
                    disabled=move || clear_expiry.get()
                    on:input=move |ev| expires_date.set(event_target_value(&ev))
                />
            </div>

            <ToggleField id="form-clear-expiry" signal=clear_expiry label=ts!(NeverExpires) />
            <Show when=move || is_edit>
                <ToggleField id="form-enabled" signal=enabled label=ts!(StatusEnabled) />
            </Show>
        </FormModalShell>
    }
}

#[component]
fn ApiKeyDetailModal(
    key: Field<ApiKey>,
    on_close: impl Fn() + 'static + Clone + Send,
) -> impl IntoView {
    let key_id = key.id().get();
    let key_name = key.name().get();
    let key_value = key.key().get();
    let key_enabled = key.enabled().get();
    let key_created = key.created_at().get();
    let key_updated = key.updated_at().get();
    let key_expires = key.expires_at().get();

    view! {
        <ModalShell
            on_close=on_close.clone()
            title=trs!(DetailTitle, &[("entity", &ts!(ApiKey))])
            card_class="max-w-lg"
        >
            <div class=CLASS_DETAIL_DIVIDER>
                <DetailRow label="ID".to_string()>{key_id}</DetailRow>
                <DetailRow label=ts!(Name)>{key_name}</DetailRow>
                <DetailRow label=ts!(ApiKey) value_class=CLASS_DETAIL_VALUE_MONO>
                    {key_value}
                </DetailRow>
                <DetailRow label=ts!(TableStatus) value_class=CLASS_DETAIL_VALUE_TAG>
                    {status_badge(key_enabled)}
                </DetailRow>
                <DetailRow label=ts!(ExpiresAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {expires_at_display(key_expires)}
                </DetailRow>
                <DetailRow label=ts!(CreatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(key_created)}
                </DetailRow>
                <DetailRow label=ts!(UpdatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(key_updated)}
                </DetailRow>
            </div>
            <DetailCloseButton on_close />
        </ModalShell>
    }
}
