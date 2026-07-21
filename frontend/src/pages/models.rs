use leptos::prelude::*;
use reactive_graph::traits::{Get, Set, Write};
use reactive_stores::{Field, Patch, Store};

use crate::api;
use crate::api::{Model, ModelStoreFields};
use crate::components::error_display::ErrorCard;
use crate::components::modal::{DeleteConfirmModal, FormModalShell, ModalShell};
use crate::components::skeleton::table_skeleton;
use crate::components::style::{
    CLASS_BORDER_B, CLASS_DETAIL_DIVIDER, CLASS_DETAIL_VALUE_PLAIN, CLASS_DETAIL_VALUE_TAG,
    CLASS_DISABLED_INPUT, CLASS_ICON_BTN, CLASS_INPUT, CLASS_LABEL, CLASS_PAGE_TITLE,
    CLASS_TEXT_MUTED,
};
use crate::components::table::{
    DataTableCard, DetailCloseButton, DetailRow, EntityModal, ToggleField, attach_save_effect,
    provider_display_name, status_badge, timestamp_str,
};
use crate::components::toast::use_toast;
use crate::components::use_page_title;
use crate::{t, tr, trs, ts};

#[derive(Store, Patch, Default)]
struct ModelsStore {
    #[store(key: String = |m: &Model| m.name.clone())]
    items: Vec<Model>,
    error: Option<String>,
}

type ModelModal = EntityModal<Model>;

#[component]
pub fn ModelsPage() -> impl IntoView {
    use_page_title(&format!("Ait - {}", ts!(Models)));
    let modal = RwSignal::new(ModelModal::Closed);
    let state = Store::new(ModelsStore::default());

    let providers_resource = LocalResource::new(|| async move {
        let providers = api::fetch_providers().await.unwrap_or_else(|e| {
            leptos::logging::warn!("failed to fetch providers: {e}");
            vec![]
        });
        providers
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect::<Vec<_>>()
    });

    let models_rsc =
        LocalResource::new(|| async move { api::fetch_models().await.map_err(|e| e.to_string()) });

    let _sync_store = Effect::new(move |_| match models_rsc.get() {
        Some(Ok(items)) => {
            state.items().patch(items);
            state.error().set(None);
        }
        Some(Err(ref e)) => state.error().set(Some(e.to_string())),
        None => {}
    });

    let on_close = move || modal.set(ModelModal::Closed);

    let do_refetch = move || models_rsc.refetch();

    view! {
        <h1 class=CLASS_PAGE_TITLE>{tr!(ListTitle, &[("entity", &t!(Models)())])}</h1>
        <DataTableCard
            item_count=Signal::derive(move || state.items().get().len())
            on_refresh=do_refetch
            on_add=move || modal.set(ModelModal::Add)
            add_label=trs!(Add, &[("entity", &ts!(Models))])
        >
            <Transition fallback=move || table_skeleton()>
                {move || match models_rsc.get() {
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
                                            )>{t!(Providers)}</th>
                                            <th class=format!(
                                                "px-6 py-3 text-left {} font-medium",
                                                CLASS_TEXT_MUTED,
                                            )>{t!(UpstreamModel)}</th>
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
                                            key=|row| row.clone().name().get()
                                            let:model
                                        >
                                            <tr class="odd:bg-gray-50 dark:odd:bg-gray-800/50">
                                                {
                                                    let m: Field<Model> = model.into();
                                                    view! {
                                                        <td class="px-6 py-4 font-medium text-gray-800 dark:text-gray-200">
                                                            {m.name().get()}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-600 dark:text-gray-400">
                                                            {move || {
                                                                providers_resource
                                                                    .get()
                                                                    .map(|ref pairs| {
                                                                        provider_display_name(&m.provider_id().get(), pairs)
                                                                            .to_string()
                                                                    })
                                                                    .unwrap_or_else(|| m.provider_id().get())
                                                            }}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-600 dark:text-gray-400">
                                                            {move || m.upstream_model().get()}
                                                        </td>
                                                        <td class="px-6 py-4">
                                                            {move || status_badge(m.enabled().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-sm">
                                                            {move || timestamp_str(m.updated_at().get())}
                                                        </td>
                                                        <td class="px-6 py-4 text-center whitespace-nowrap">
                                                            <div class="flex items-center justify-center gap-3">
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ModelModal::Detail(m))
                                                                >
                                                                    <i class="fas fa-eye text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ModelModal::Edit(m))
                                                                >
                                                                    <i class="fas fa-pen text-xs"></i>
                                                                </button>
                                                                <button
                                                                    class=CLASS_ICON_BTN
                                                                    on:click=move |_| modal.set(ModelModal::Delete(m))
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

        {move || match modal.get() {
            ModelModal::Add => {
                view! { <ModelFormModal providers_resource state on_close /> }.into_any()
            }
            ModelModal::Edit(model) => {
                view! { <ModelFormModal providers_resource state on_close edit_model=model /> }
                    .into_any()
            }
            ModelModal::Delete(model) => {
                let model_name = model.name().get();
                let item_name = model_name.clone();
                let action: Action<(), Result<(), String>> = Action::new_local({
                    let name = model_name.clone();
                    move |_: &()| {
                        let n = name.clone();
                        async move { api::delete_model(&n).await.map_err(|e| e.to_string()) }
                    }
                });
                let on_success = move || {
                    state.items().write().retain(|m| m.name != model_name);
                    on_close();
                };
                view! {
                    <DeleteConfirmModal
                        entity_name=Box::new(t!(Models))
                        item_name
                        action
                        on_close
                        on_success
                    />
                }
                    .into_any()
            }
            ModelModal::Detail(model) => {
                view! { <ModelDetailModal model providers_resource on_close /> }.into_any()
            }
            ModelModal::Closed => ().into_any(),
        }}
    }
}

#[derive(Clone, Debug)]
struct ModelFormData {
    name: String,
    provider_id: String,
    upstream_model: String,
    enabled: bool,
}

#[component]
fn ModelFormModal(
    providers_resource: LocalResource<Vec<(String, String)>>,
    state: Store<ModelsStore>,
    on_close: impl Fn() + 'static + Clone + Send,
    #[prop(optional)] edit_model: Option<Field<Model>>,
) -> impl IntoView {
    let is_edit = edit_model.is_some();
    let edit_name = edit_model.map(|f| f.name().get());

    let name = RwSignal::new(edit_name.clone().unwrap_or_default());
    let provider_id = RwSignal::new(
        edit_model
            .map(|f| f.provider_id().get())
            .or_else(|| {
                providers_resource
                    .get_untracked()
                    .and_then(|providers| providers.first().map(|(id, _)| id.clone()))
            })
            .unwrap_or_default(),
    );
    let upstream_model = RwSignal::new(
        edit_model
            .map(|f| f.upstream_model().get())
            .unwrap_or_default(),
    );
    let enabled = RwSignal::new(edit_model.map(|f| f.enabled().get()).unwrap_or(true));
    let form_error = RwSignal::new(String::new());

    let title = if is_edit {
        trs!(Edit, &[("entity", &ts!(Models))])
    } else {
        trs!(Add, &[("entity", &ts!(Models))])
    };

    let save_action: Action<ModelFormData, Result<Model, String>> = Action::new_local({
        let edit_name = edit_name.clone();
        move |input: &ModelFormData| {
            let data = input.clone();
            let edit_name = edit_name.clone();
            async move {
                if let Some(name) = &edit_name {
                    api::update_model(name, &data.provider_id, &data.upstream_model, data.enabled)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    api::create_model(
                        &data.name,
                        &data.provider_id,
                        &data.upstream_model,
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
                &[("entity", &ts!(Models)), ("action", &action_label)]
            ));
            on_submitted();
        },
    );

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if save_action.pending().get_untracked() {
            return;
        }
        let model_name = name.get_untracked();
        let model_provider = provider_id.get_untracked();
        if model_name.is_empty() || model_provider.is_empty() {
            form_error.set(ts!(NameAndProviderIdRequired));
            return;
        }
        form_error.set(String::new());

        let model_upstream = upstream_model.get_untracked();
        let model_enabled = enabled.get_untracked();

        save_action.dispatch(ModelFormData {
            name: model_name,
            provider_id: model_provider,
            upstream_model: model_upstream,
            enabled: model_enabled,
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
                <Show when=move || !is_edit>
                    <input
                        id="form-name"
                        type="text"
                        class=CLASS_INPUT
                        placeholder=ts!(Name)
                        prop:value=name
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                </Show>
                <Show when=move || is_edit>
                    <input
                        id="form-name"
                        type="text"
                        class=CLASS_DISABLED_INPUT
                        prop:value=name
                        disabled
                    />
                </Show>
            </div>

            <div>
                <label for="form-provider" class=CLASS_LABEL>
                    {t!(Providers)}
                </label>
                <select
                    id="form-provider"
                    class=CLASS_INPUT
                    on:change=move |ev| provider_id.set(event_target_value(&ev))
                >
                    {providers_resource
                        .get_untracked()
                        .map(|providers| {
                            let current = provider_id.get_untracked();
                            providers
                                .iter()
                                .map(|(id, name)| {
                                    view! {
                                        <option value=id.clone() selected=current == *id>
                                            {name.clone()}
                                        </option>
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()}
                </select>
            </div>

            <div>
                <label for="form-upstream" class=CLASS_LABEL>
                    {t!(UpstreamModel)}
                </label>
                <input
                    id="form-upstream"
                    type="text"
                    class=CLASS_INPUT
                    placeholder=ts!(UpstreamModel)
                    prop:value=upstream_model
                    on:input=move |ev| upstream_model.set(event_target_value(&ev))
                />
            </div>

            <ToggleField id="form-enabled" signal=enabled label=ts!(StatusEnabled) />
        </FormModalShell>
    }
}

#[component]
fn ModelDetailModal(
    model: Field<Model>,
    providers_resource: LocalResource<Vec<(String, String)>>,
    on_close: impl Fn() + 'static + Clone + Send,
) -> impl IntoView {
    let model_id = model.id().get();
    let model_name = model.name().get();
    let model_upstream = model.upstream_model().get();
    let model_enabled = model.enabled().get();
    let model_created = model.created_at().get();
    let model_updated = model.updated_at().get();
    let provider_name = providers_resource
        .get_untracked()
        .map(|ref pairs| provider_display_name(&model.provider_id().get(), pairs).to_string())
        .unwrap_or_else(|| model.provider_id().get());

    view! {
        <ModalShell
            on_close=on_close.clone()
            title=trs!(DetailTitle, &[("entity", &ts!(Models))])
            card_class="max-w-lg"
        >
            <div class=CLASS_DETAIL_DIVIDER>
                <DetailRow label="ID".to_string()>{model_id}</DetailRow>
                <DetailRow label=ts!(Name)>{model_name}</DetailRow>
                <DetailRow label=ts!(Providers)>{provider_name}</DetailRow>
                <DetailRow label=ts!(UpstreamModel)>{model_upstream}</DetailRow>
                <DetailRow label=ts!(TableStatus) value_class=CLASS_DETAIL_VALUE_TAG>
                    {status_badge(model_enabled)}
                </DetailRow>
                <DetailRow label=ts!(CreatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(model_created)}
                </DetailRow>
                <DetailRow label=ts!(UpdatedAt) value_class=CLASS_DETAIL_VALUE_PLAIN>
                    {timestamp_str(model_updated)}
                </DetailRow>
            </div>
            <DetailCloseButton on_close />
        </ModalShell>
    }
}
