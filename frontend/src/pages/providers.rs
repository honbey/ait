use std::collections::HashMap;

use leptos::prelude::*;
use reactive_graph::traits::{Get, Read, ReadUntracked, Set, Write};
use reactive_stores::{Field, Patch, Store};

use crate::api;
use crate::api::{Provider, ProviderStoreFields};

use crate::components::error_display::ErrorCard;
use crate::components::modal::{DeleteConfirmModal, FormModalShell, ModalShell};
use crate::components::skeleton::table_skeleton;
use crate::components::style::{
    CLASS_BORDER_B, CLASS_DETAIL_DIVIDER, CLASS_DETAIL_VALUE_MONO, CLASS_DETAIL_VALUE_PLAIN,
    CLASS_DETAIL_VALUE_TAG, CLASS_ICON_BTN, CLASS_INPUT, CLASS_LABEL, CLASS_PAGE_TITLE,
    CLASS_TEXT_MUTED,
};
use crate::components::table::{
    DataTableCard, DetailCloseButton, DetailRow, EntityModal, ToggleField, attach_save_effect,
    provider_display_name, status_badge, timestamp_str,
};
use crate::components::toast::use_toast;
use crate::components::use_page_title;
use crate::storage;
use crate::{t, tr, trs, ts};
use leptos::logging;

const PT_KEY: &str = "ait_provider_types";
const PT_TS_KEY: &str = "ait_provider_types_ts";
const PT_TTL_MS: f64 = 3_600_000.0;

type ProviderModal = EntityModal<Provider>;

#[derive(Store, Patch, Default)]
struct ProvidersStore {
    #[store(key: String = |p: &Provider| p.id.clone())]
    items: Vec<Provider>,
    error: Option<String>,
}

#[component]
pub fn ProvidersPage() -> impl IntoView {
    use_page_title(move || format!("{} - Ait", t!(Providers)()));
    let modal = RwSignal::new(ProviderModal::Closed);
    let state = Store::new(ProvidersStore::default());

    let provider_types_resource = LocalResource::new(|| async move {
        let cached: Option<Vec<(String, String)>> = storage::get_item(PT_TS_KEY)
            .and_then(|ts| ts.parse::<f64>().ok())
            .filter(|ts| js_sys::Date::now() - ts < PT_TTL_MS)
            .and_then(|_| storage::get_item(PT_KEY))
            .and_then(|json| serde_json::from_str(&json).ok());
        if let Some(cached) = cached {
            return cached;
        }
        match api::fetch_provider_types().await {
            Ok(types) => {
                let pairs: Vec<(String, String)> = types
                    .into_iter()
                    .map(|t| (t.provider_type, t.display_name))
                    .collect();
                if let Ok(json) = serde_json::to_string(&pairs) {
                    storage::set_item(PT_KEY, &json);
                    storage::set_item(PT_TS_KEY, &js_sys::Date::now().to_string());
                }
                pairs
            }
            Err(e) => {
                logging::warn!("failed to fetch provider types: {e}");
                vec![]
            }
        }
    });

    let providers_rsc = LocalResource::new(|| async move { api::fetch_providers().await });

    let _sync_store = Effect::new(move |_| match providers_rsc.get() {
        Some(Ok(items)) => {
            state.items().patch(items);
            state.error().set(None);
        }
        Some(Err(ref e)) => state.error().set(Some(e.to_string())),
        None => {}
    });

    let on_close = move || modal.set(ProviderModal::Closed);

    // One type-id->name map instead of cloning and linearly scanning the type
    // list in every row.
    let type_names = Memo::new(move |_| {
        provider_types_resource
            .get()
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<String, String>>()
    });

    let do_refetch = move || providers_rsc.refetch();

    view! {
        <h1 class=CLASS_PAGE_TITLE>{tr!(ListTitle, &[("entity", &t!(Providers)())])}</h1>
        <DataTableCard
            item_count=Signal::derive(move || state.items().read().len())
            on_refresh=do_refetch
            on_add=move || modal.set(ProviderModal::Add)
            add_label=trs!(Add, &[("entity", &ts!(Providers))])
        >
            <Transition fallback=move || table_skeleton()>
                {move || match providers_rsc.get() {
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
                                            )>{t!(ProviderType)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(ProviderBaseUrl)}</th>
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
                                            key=|row| row.clone().id().read_untracked().to_owned()
                                            let:provider
                                        >
                                            <tr class="odd:bg-gray-50 dark:odd:bg-gray-800/50">
                                                {
                                                    let p: Field<Provider> = provider.into();
                                                    view! {
                                                        <td class="px-6 py-4 font-medium text-gray-800 dark:text-gray-200">
                                                            {move || p.name().get()}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-600 dark:text-gray-400">
                                                            {move || {
                                                                type_names
                                                                    .get()
                                                                    .get(&p.provider_type().get())
                                                                    .cloned()
                                                                    .unwrap_or_else(|| p.provider_type().get())
                                                            }}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono">
                                                            {move || p.base_url().get()}
                                                        </td>
                                                        <td class="px-6 py-4">
                                                            {move || status_badge(p.enabled().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-sm">
                                                            {move || timestamp_str(p.updated_at().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-center whitespace-nowrap">
                                                            <div class="flex items-center justify-center gap-3">
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ProviderModal::Detail(p))
                                                                >
                                                                    <i class="fas fa-eye text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ProviderModal::Edit(p))
                                                                >
                                                                    <i class="fas fa-pen text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ProviderModal::Delete(p))
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
                        view! {
                            <ErrorCard
                                message=e.message.clone()
                                request_id=e.request_id.clone()
                                on_retry=Box::new(do_refetch)
                            />
                        }
                            .into_any()
                    }
                    None => ().into_any(),
                }}
            </Transition>
        </DataTableCard>

        {move || match modal.get() {
            ProviderModal::Add => {
                view! { <ProviderFormModal provider_types_resource state on_close /> }.into_any()
            }
            ProviderModal::Edit(prov) => {
                view! {
                    <ProviderFormModal provider_types_resource state on_close edit_model=prov />
                }
                    .into_any()
            }
            ProviderModal::Delete(provider) => {
                let prov_id = provider.id().read_untracked().to_owned();
                let item_id = prov_id.clone();
                let action: Action<(), Result<(), String>> = Action::new_local({
                    let pid = prov_id.clone();
                    move |_: &()| {
                        let n = pid.clone();
                        async move { api::delete_provider(&n).await.map_err(|e| e.to_string()) }
                    }
                });
                let on_success = move || {
                    state.items().write().retain(|p| p.id != prov_id);
                    on_close();
                };
                view! {
                    <DeleteConfirmModal
                        entity_name=Box::new(t!(Providers))
                        item_name=item_id
                        action
                        on_close
                        on_success
                    />
                }
                    .into_any()
            }
            ProviderModal::Detail(prov) => {
                view! { <ProviderDetailModal provider=prov provider_types_resource on_close /> }
                    .into_any()
            }
            ProviderModal::Closed => ().into_any(),
        }}
    }
}

#[derive(Clone, Debug)]
struct ProviderFormData {
    name: String,
    provider_type: String,
    base_url: String,
    api_key: Option<String>,
    enabled: bool,
}

#[component]
fn ProviderFormModal(
    provider_types_resource: LocalResource<Vec<(String, String)>>,
    state: Store<ProvidersStore>,
    on_close: impl Fn() + 'static + Clone + Send,
    #[prop(optional)] edit_model: Option<Field<Provider>>,
) -> impl IntoView {
    let is_edit = edit_model.is_some();
    let edit_id = edit_model.map(|f| f.id().read_untracked().to_owned());

    let name = RwSignal::new(
        edit_model
            .map(|f| f.name().read_untracked().to_owned())
            .unwrap_or_default(),
    );
    let provider_type = RwSignal::new(
        edit_model
            .map(|f| f.provider_type().read_untracked().to_owned())
            .or_else(|| {
                provider_types_resource
                    .get_untracked()
                    .and_then(|types| types.first().map(|(id, _)| id.clone()))
            })
            .unwrap_or_else(|| "openai_compat".to_string()),
    );
    let base_url = RwSignal::new(
        edit_model
            .map(|f| f.base_url().read_untracked().to_owned())
            .unwrap_or_else(|| "https://".to_string()),
    );
    let api_key = RwSignal::new(String::new());
    let enabled = RwSignal::new(
        edit_model
            .map(|f| *f.enabled().read_untracked())
            .unwrap_or(true),
    );
    let clear_key = RwSignal::new(false);
    let form_error = RwSignal::new(String::new());

    let title = if is_edit {
        trs!(Edit, &[("entity", &ts!(Providers))])
    } else {
        trs!(Add, &[("entity", &ts!(Providers))])
    };

    let save_action: Action<ProviderFormData, Result<Provider, String>> = Action::new_local({
        let edit_id = edit_id.clone();
        move |input: &ProviderFormData| {
            let data = input.clone();
            let edit_id = edit_id.clone();
            async move {
                if let Some(id) = &edit_id {
                    api::update_provider(
                        id,
                        &data.name,
                        &data.provider_type,
                        &data.base_url,
                        data.api_key.as_deref(),
                        data.enabled,
                    )
                    .await
                    .map_err(|e| e.to_string())
                } else {
                    api::create_provider(
                        &data.name,
                        &data.provider_type,
                        &data.base_url,
                        data.api_key.as_deref(),
                        data.enabled,
                    )
                    .await
                    .map_err(|e| e.to_string())
                }
            }
        }
    });

    let on_submitted = on_close.clone();
    let toast = use_toast();

    attach_save_effect(
        &save_action,
        edit_model,
        state.items(),
        form_error,
        move || {
            let action_label = if is_edit {
                ts!(ActionUpdated)
            } else {
                ts!(ActionCreated)
            };
            toast.success(trs!(
                EntityAction,
                &[("entity", &ts!(Providers)), ("action", &action_label)]
            ));
            on_submitted();
        },
    );

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if save_action.pending().get_untracked() {
            return;
        }
        let provider_name = name.get_untracked();
        let provider_url = base_url.get_untracked();
        if provider_name.is_empty() || provider_url.is_empty() {
            form_error.set(ts!(NameAndBaseUrlRequired));
            return;
        }
        if !provider_url.starts_with("http://") && !provider_url.starts_with("https://") {
            form_error.set(ts!(BaseUrlInvalid));
            return;
        }
        if !provider_url
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '~' | ':' | '/'))
        {
            form_error.set(ts!(BaseUrlInvalid));
            return;
        }
        form_error.set(String::new());

        let ptype = provider_type.get_untracked();
        let api_key_value = {
            let raw = api_key.get_untracked();
            if is_edit {
                if !raw.is_empty() {
                    Some(raw)
                } else if clear_key.get_untracked() {
                    Some(String::new())
                } else {
                    None
                }
            } else if raw.is_empty() {
                None
            } else {
                Some(raw)
            }
        };
        let provider_enabled = enabled.get_untracked();

        save_action.dispatch(ProviderFormData {
            name: provider_name,
            provider_type: ptype,
            base_url: provider_url,
            api_key: api_key_value,
            enabled: provider_enabled,
        });
    };

    view! {
        <FormModalShell
            on_close=on_close.clone()
            title=title
            on_submit
            pending=save_action.pending()
            is_edit
            form_error
        >

            <div>
                <label for="form-name" class=CLASS_LABEL>
                    {t!(Name)}
                </label>
                <input
                    id="form-name"
                    type="text"
                    class=CLASS_INPUT
                    placeholder=ts!(Name)
                    prop:value=name
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
            </div>

            <div>
                <label for="form-type" class=CLASS_LABEL>
                    {t!(ProviderType)}
                </label>
                <select
                    id="form-type"
                    class=CLASS_INPUT
                    on:change=move |ev| provider_type.set(event_target_value(&ev))
                >
                    {move || {
                        let current = provider_type.get_untracked();
                        let types = provider_types_resource.get().filter(|types| !types.is_empty());
                        if let Some(types) = types {
                            types
                                .iter()
                                .map(|(id, name)| {
                                    view! {
                                        <option value=id.clone() selected=current == *id>
                                            {name.clone()}
                                        </option>
                                    }
                                })
                                .collect::<Vec<_>>()
                        } else {
                            vec![
                                view! {
                                    <option
                                        value="openai_compat".to_string()
                                        selected=current == "openai_compat"
                                    >
                                        {"OpenAI Compatible".to_string()}
                                    </option>
                                },
                            ]
                        }
                    }}
                </select>
            </div>

            <div>
                <label for="form-url" class=CLASS_LABEL>
                    {t!(ProviderBaseUrl)}
                </label>
                <input
                    id="form-url"
                    type="text"
                    class=CLASS_INPUT
                    placeholder=ts!(ProviderBaseUrl)
                    prop:value=base_url
                    on:input=move |ev| base_url.set(event_target_value(&ev))
                />
            </div>

            <div>
                <label for="form-api-key" class=CLASS_LABEL>
                    {t!(ApiKey)}
                    <Show when=move || is_edit>
                        <span class="text-xs text-gray-400 dark:text-gray-500 ml-1">
                            {t!(KeepKeyHint)}
                        </span>
                    </Show>
                </label>
                <input
                    id="form-api-key"
                    type="password"
                    autocomplete="off"
                    class=CLASS_INPUT
                    placeholder=ts!(ApiKey)
                    prop:value=api_key
                    on:input=move |ev| api_key.set(event_target_value(&ev))
                />
            </div>

            <Show when=move || is_edit>
                <ToggleField id="form-clear-key" signal=clear_key label=ts!(ClearKey) />
            </Show>

            <ToggleField id="form-enabled" signal=enabled label=ts!(StatusEnabled) />
        </FormModalShell>
    }
}

#[component]
fn ProviderDetailModal(
    provider: Field<Provider>,
    provider_types_resource: LocalResource<Vec<(String, String)>>,
    on_close: impl Fn() + 'static + Clone + Send,
) -> impl IntoView {
    let prov_id = provider.id().read_untracked().to_owned();
    let prov_name = provider.name().read_untracked().to_owned();
    let prov_base_url = provider.base_url().read_untracked().to_owned();
    let prov_provider_type = provider.provider_type().read_untracked().to_owned();
    let prov_enabled = *provider.enabled().read_untracked();
    let prov_created = *provider.created_at().read_untracked();
    let prov_updated = *provider.updated_at().read_untracked();
    let prov_api_key = provider.api_key().read_untracked().to_owned();
    let api_key_display = prov_api_key.as_deref().unwrap_or("—").to_string();
    let type_name = provider_types_resource
        .get_untracked()
        .map(|types| provider_display_name(&prov_provider_type, &types).to_string())
        .unwrap_or_else(|| prov_provider_type);

    view! {
        <ModalShell
            on_close=on_close.clone()
            title=trs!(DetailTitle, &[("entity", &ts!(Providers))])
            card_class="max-w-lg"
        >
            <div class=CLASS_DETAIL_DIVIDER>
                <DetailRow label="ID".to_string()>{prov_id}</DetailRow>
                <DetailRow label=ts!(Name)>{prov_name}</DetailRow>
                <DetailRow label=ts!(ProviderType)>{type_name}</DetailRow>
                <DetailRow label=ts!(ProviderBaseUrl)>{prov_base_url}</DetailRow>
                <DetailRow label=ts!(ApiKey) value_class=CLASS_DETAIL_VALUE_MONO>
                    {api_key_display}
                </DetailRow>
                <DetailRow label=ts!(TableStatus) value_class=CLASS_DETAIL_VALUE_TAG>
                    {status_badge(prov_enabled)}
                </DetailRow>
                <DetailRow label=ts!(CreatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(prov_created)}
                </DetailRow>
                <DetailRow label=ts!(UpdatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(prov_updated)}
                </DetailRow>
            </div>
            <DetailCloseButton on_close />
        </ModalShell>
    }
}
