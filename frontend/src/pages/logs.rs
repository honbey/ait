use leptos::prelude::*;

use crate::api::{self, PaginatedResponse, ProxyLogEntryResponse};
use crate::components::error_display::ErrorCard;
use crate::components::modal::ModalShell;
use crate::components::skeleton::table_skeleton;
use crate::components::style::{
    CLASS_BTN_PRIMARY, CLASS_DETAIL_DIVIDER, CLASS_DETAIL_VALUE, CLASS_DETAIL_VALUE_MONO,
    CLASS_DETAIL_VALUE_PLAIN, CLASS_INPUT, CLASS_PAGE_TITLE,
};
use crate::components::table::DetailRow;
use crate::components::table::timestamp_str;
use crate::time_utils::{date_str_to_ts, ts_to_date_str};
use crate::{t, ts};

fn latency_pill(latency_s: f64) -> &'static str {
    if latency_s < 3.0 {
        "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
    } else {
        "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
    }
}

fn grey_pill() -> &'static str {
    "bg-gray-100 text-gray-600 dark:bg-gray-600 dark:text-gray-300"
}

fn latency_s(ms: i64) -> f64 {
    (ms as f64) / 1000.0
}

#[component]
fn PaginationBar(page: RwSignal<u64>, total_signal: Signal<u64>, per_page: u64) -> impl IntoView {
    let input_ref = NodeRef::<leptos::html::Input>::new();

    let go = move |p: u64| {
        let t = total_signal.get_untracked();
        if p >= 1 && p <= t.div_ceil(per_page).max(1) {
            page.set(p);
        }
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            if let Ok(p) = event_target_value(&ev).parse::<u64>() {
                go(p);
            }
            let _ = input_ref
                .get_untracked()
                .map(|el| el.set_value(&page.get_untracked().to_string()));
        }
    };

    let on_blur = move |ev: leptos::ev::FocusEvent| {
        if let Ok(p) = event_target_value(&ev).parse::<u64>() {
            go(p);
        }
        let cur = page.get_untracked();
        let _ = input_ref
            .get_untracked()
            .map(|el| el.set_value(&cur.to_string()));
    };

    move || {
        let t = total_signal.get();
        let total_pages = t.div_ceil(per_page).max(1);
        let cur = page.get();
        let is_first = cur == 1;
        let is_last = cur == total_pages;

        view! {
            <div class="flex items-center justify-between px-6 py-4 border-t border-gray-100 dark:border-gray-700 text-sm text-gray-500 dark:text-gray-400">
                <div>
                    {move || {
                        let t = total_signal.get();
                        let start = (cur - 1) * per_page + 1;
                        let end = (start + per_page - 1).min(t);
                        if t == 0 {
                            String::new()
                        } else {
                            crate::i18n::use_i18n()
                                .t_replace(
                                    crate::i18n::K::PaginationRange,
                                    &[
                                        ("start", &start.to_string()),
                                        ("end", &end.to_string()),
                                        ("total", &t.to_string()),
                                    ],
                                )
                        }
                    }}
                </div>
                <div class="flex items-center gap-1">
                    <button
                        class=if is_first {
                            "px-2 py-1 text-sm rounded text-gray-300 dark:text-gray-600 cursor-not-allowed"
                        } else {
                            "px-2 py-1 text-sm rounded text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer"
                        }
                        disabled=is_first
                        on:click=move |_| go(1)
                    >
                        <i class="fas fa-angles-left"></i>
                    </button>
                    <button
                        class=if is_first {
                            "px-2 py-1 text-sm rounded text-gray-300 dark:text-gray-600 cursor-not-allowed"
                        } else {
                            "px-2 py-1 text-sm rounded text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer"
                        }
                        disabled=is_first
                        on:click=move |_| go(cur - 1)
                    >
                        <i class="fas fa-chevron-left"></i>
                    </button>
                    <input
                        node_ref=input_ref
                        id="filter-page"
                        class="w-10 text-center px-1 py-1 text-sm rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 outline-none"
                        prop:value=move || page.get().to_string()
                        on:keydown=on_keydown
                        on:blur=on_blur
                    />
                    <span class="text-sm text-gray-500 dark:text-gray-400">
                        {" / "}{total_pages}
                    </span>
                    <button
                        class=if is_last {
                            "px-2 py-1 text-sm rounded text-gray-300 dark:text-gray-600 cursor-not-allowed"
                        } else {
                            "px-2 py-1 text-sm rounded text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer"
                        }
                        disabled=is_last
                        on:click=move |_| go(cur + 1)
                    >
                        <i class="fas fa-chevron-right"></i>
                    </button>
                    <button
                        class=if is_last {
                            "px-2 py-1 text-sm rounded text-gray-300 dark:text-gray-600 cursor-not-allowed"
                        } else {
                            "px-2 py-1 text-sm rounded text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer"
                        }
                        disabled=is_last
                        on:click=move |_| go(total_pages)
                    >
                        <i class="fas fa-angles-right"></i>
                    </button>
                </div>
            </div>
        }
    }
}

#[component]
pub fn LogsPage() -> impl IntoView {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        doc.set_title(&format!("Ait - {}", ts!(LogQuery)));
    }

    let page = RwSignal::new(1u64);
    let query_trigger = RwSignal::new(0u64);
    let per_page: u64 = 10;

    let (start_ts, set_start_ts) = signal::<Option<i64>>(None);
    let (end_ts, set_end_ts) = signal::<Option<i64>>(None);
    let (provider_name, set_provider_name) = signal(String::new());
    let (model_name, set_model_name) = signal(String::new());
    let (api_key_name, set_api_key_name) = signal(String::new());
    let (client_ip, set_client_ip) = signal(String::new());
    let (status, set_status) = signal(String::new());
    let (endpoint, set_endpoint) = signal(String::new());
    let (is_streaming, set_is_streaming) = signal::<Option<bool>>(None);

    let start_str = Signal::derive(move || start_ts.get().map(ts_to_date_str).unwrap_or_default());
    let end_str = Signal::derive(move || end_ts.get().map(ts_to_date_str).unwrap_or_default());

    let detail_item = RwSignal::new(None::<ProxyLogEntryResponse>);

    let rsc: LocalResource<Result<PaginatedResponse<ProxyLogEntryResponse>, String>> =
        LocalResource::new(move || {
            let p = page.get();
            let _ = query_trigger.get();

            let s = start_ts.get_untracked();
            let e = end_ts.get_untracked();
            let pn = provider_name.get_untracked();
            let mn = model_name.get_untracked();
            let ak = api_key_name.get_untracked();
            let ci = client_ip.get_untracked();
            let st = status.get_untracked();
            let ep = endpoint.get_untracked();
            let ist = is_streaming.get_untracked();

            async move {
                api::fetch_proxy_logs(
                    p,
                    per_page,
                    s,
                    e,
                    if pn.is_empty() { None } else { Some(pn) },
                    if mn.is_empty() { None } else { Some(mn) },
                    if ak.is_empty() { None } else { Some(ak) },
                    if ci.is_empty() { None } else { Some(ci) },
                    if st.is_empty() { None } else { Some(st) },
                    if ep.is_empty() { None } else { Some(ep) },
                    ist,
                )
                .await
                .map_err(|e| e.to_string())
            }
        });

    let total: Signal<u64> =
        Signal::derive(move || rsc.get().and_then(|r| r.ok()).map(|r| r.total).unwrap_or(0));

    let do_query = move |_| {
        query_trigger.set(query_trigger.get_untracked() + 1);
        page.set(1);
    };

    let do_reset = move |_| {
        set_start_ts.set(None);
        set_end_ts.set(None);
        set_provider_name.set(String::new());
        set_model_name.set(String::new());
        set_api_key_name.set(String::new());
        set_client_ip.set(String::new());
        set_status.set(String::new());
        set_endpoint.set(String::new());
        set_is_streaming.set(None);
        page.set(1);
    };

    let on_start_date = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        set_start_ts.set(date_str_to_ts(&val));
    };

    let on_end_date = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        set_end_ts.set(date_str_to_ts(&val));
    };

    let select_cls = "w-full px-3 py-2 border border-gray-300 dark:border-gray-600 \
        rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 \
        focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none";

    view! {
        <div class="space-y-6">
            <div class="flex items-center justify-between">
                <h1 class=CLASS_PAGE_TITLE style="margin-bottom: 0">
                    {t!(LogQuery)}
                </h1>
                <div class="flex items-center gap-2 shrink-0">
                    <select
                        id="filter-source"
                        title=t!(LogSource)
                        disabled
                        class=format!(
                            "{} cursor-not-allowed bg-gray-100 dark:bg-gray-600",
                            select_cls,
                        )
                    >
                        <option value="proxy-log" selected>
                            {"proxy-log"}
                        </option>
                    </select>
                </div>
            </div>

            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm p-5 space-y-4">
                <div class="grid grid-cols-8 gap-3 items-end">
                    <div class="col-span-2">
                        <label
                            for="filter-provider-name"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(Providers)}
                        </label>
                        <input
                            id="filter-provider-name"
                            type="text"
                            class=CLASS_INPUT
                            prop:value=move || provider_name.get()
                            on:input=move |ev| set_provider_name.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="col-span-2">
                        <label
                            for="filter-model-name"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(Models)}
                        </label>
                        <input
                            id="filter-model-name"
                            type="text"
                            class=CLASS_INPUT
                            prop:value=move || model_name.get()
                            on:input=move |ev| set_model_name.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="col-span-2">
                        <label
                            for="filter-api-key-name"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(LogApiKeyName)}
                        </label>
                        <input
                            id="filter-api-key-name"
                            type="text"
                            class=CLASS_INPUT
                            prop:value=move || api_key_name.get()
                            on:input=move |ev| set_api_key_name.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="col-span-1">
                        <label
                            for="filter-client-ip"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(LogClientIp)}
                        </label>
                        <input
                            id="filter-client-ip"
                            type="text"
                            class=CLASS_INPUT
                            prop:value=move || client_ip.get()
                            on:input=move |ev| set_client_ip.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="col-span-1">
                        <label
                            for="filter-status"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(TableStatus)}
                        </label>
                        <input
                            id="filter-status"
                            type="text"
                            class=CLASS_INPUT
                            prop:value=move || status.get()
                            on:input=move |ev| set_status.set(event_target_value(&ev))
                        />
                    </div>
                </div>

                <div class="flex items-center gap-3 flex-wrap">
                    <div class="w-1/6 min-w-[120px]">
                        <label
                            for="filter-start-date"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(StartDate)}
                        </label>
                        <input
                            id="filter-start-date"
                            type="date"
                            class=CLASS_INPUT
                            prop:value=move || start_str.get()
                            on:change=on_start_date
                        />
                    </div>
                    <div class="w-1/6 min-w-[120px]">
                        <label
                            for="filter-end-date"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(EndDate)}
                        </label>
                        <input
                            id="filter-end-date"
                            type="date"
                            class=CLASS_INPUT
                            prop:value=move || end_str.get()
                            on:change=on_end_date
                        />
                    </div>
                    <div class="w-48">
                        <label
                            for="filter-endpoint"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(LogEndpoint)}
                        </label>
                        <select
                            id="filter-endpoint"
                            class=select_cls
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                set_endpoint.set(v);
                            }
                        >
                            <option value="" selected>
                                {t!(LogAll)}
                            </option>
                            <option value="/v1/chat/completions">{"/v1/chat/completions"}</option>
                            <option value="/v1/completions">{"/v1/completions"}</option>
                            <option value="/v1/embeddings">{"/v1/embeddings"}</option>
                        </select>
                    </div>
                    <div class="w-36">
                        <label
                            for="filter-streaming"
                            class="block text-xs text-gray-500 dark:text-gray-400 mb-1"
                        >
                            {t!(LogStreaming)}
                        </label>
                        <select
                            id="filter-streaming"
                            class=select_cls
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                set_is_streaming
                                    .set(
                                        match v.as_str() {
                                            "true" => Some(true),
                                            "false" => Some(false),
                                            _ => None,
                                        },
                                    );
                            }
                        >
                            <option value="" selected>
                                {t!(LogAll)}
                            </option>
                            <option value="true">{t!(LogYes)}</option>
                            <option value="false">{t!(LogNo)}</option>
                        </select>
                    </div>
                    <div class="flex-1"></div>
                    <button
                        class="px-4 py-2 text-sm font-medium rounded-lg \
                        bg-gray-100 dark:bg-gray-700 \
                        text-gray-600 dark:text-gray-300 \
                        hover:bg-gray-200 dark:hover:bg-gray-600 cursor-pointer"
                        on:click=do_reset
                    >
                        {t!(LogReset)}
                    </button>
                    <button class=CLASS_BTN_PRIMARY on:click=do_query>
                        <i class="fas fa-search"></i>
                        {t!(LogQueryBtn)}
                    </button>
                </div>
            </div>

            <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm">
                <Transition fallback=move || table_skeleton()>
                    {move || match rsc.get() {
                        Some(Ok(ref data)) => {
                            let items = &data.items;
                            if items.is_empty() {
                                view! {
                                    <div class="p-12 text-center text-gray-400 dark:text-gray-500 text-sm">
                                        {t!(LogNoData)}
                                    </div>
                                }
                                    .into_any()
                            } else {
                                let rows = items.clone();
                                view! {
                                    <>
                                        <div class="overflow-x-auto">
                                            <table class="w-full text-sm">
                                                <thead>
                                                    <tr class="border-b border-gray-100 dark:border-gray-700">
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(CreatedAt)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(Providers)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(Models)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(LogLatency)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(LogInput)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(LogOutput)}
                                                        </th>
                                                        <th class="px-6 py-3 text-left text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(LogClientIp)}
                                                        </th>
                                                        <th class="px-6 py-3 text-center text-gray-500 dark:text-gray-400 font-medium whitespace-nowrap">
                                                            {t!(TableStatus)}
                                                        </th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {rows
                                                        .iter()
                                                        .map(|entry| {
                                                            let e = entry.clone();
                                                            let entry_clone = entry.clone();
                                                            view! {
                                                                <tr
                                                                    class="odd:bg-gray-50 dark:odd:bg-gray-800/50 cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-700/50"
                                                                    on:click=move |_| detail_item.set(Some(entry_clone.clone()))
                                                                >
                                                                    <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-sm">
                                                                        {timestamp_str(e.timestamp / 1_000_000)}
                                                                    </td>
                                                                    <td class="px-6 py-4 text-gray-800 dark:text-gray-200 font-medium whitespace-nowrap">
                                                                        {e.provider_name}
                                                                    </td>
                                                                    <td class="px-6 py-4 text-gray-600 dark:text-gray-400 whitespace-nowrap">
                                                                        {e.model_name}
                                                                    </td>
                                                                    <td class="px-6 py-4 whitespace-nowrap">
                                                                        <div class="flex items-center gap-1">
                                                                            <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400">
                                                                                {format!("{:.2}s", latency_s(e.latency_ms))}
                                                                            </span>
                                                                            <Show
                                                                                when=move || e.is_streaming
                                                                                fallback=move || {
                                                                                    view! {
                                                                                        <i class="fas fa-box text-xs text-gray-300 dark:text-gray-600"></i>
                                                                                    }
                                                                                }
                                                                            >
                                                                                {{
                                                                                    let ttft = e.time_to_first_token_ms.unwrap_or(0);
                                                                                    let ttft_s = latency_s(ttft);
                                                                                    view! {
                                                                                        <span class=format!(
                                                                                            "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium {}",
                                                                                            latency_pill(ttft_s),
                                                                                        )>
                                                                                            <i class="fas fa-bolt text-xs"></i>
                                                                                            {format!("{:.2}s", ttft_s)}
                                                                                        </span>
                                                                                    }
                                                                                }}
                                                                            </Show>
                                                                        </div>
                                                                    </td>
                                                                    <td class="px-6 py-4 whitespace-nowrap">
                                                                        <span class="text-gray-800 dark:text-gray-200 font-medium">
                                                                            {e.prompt_tokens.unwrap_or(0)}
                                                                        </span>
                                                                        {if e.cached_tokens.unwrap_or(0) > 0 {
                                                                            view! {
                                                                                <span class=format!(
                                                                                    "inline-flex ml-1 px-1.5 py-0.5 rounded text-xs font-medium {}",
                                                                                    grey_pill(),
                                                                                )>{e.cached_tokens.unwrap_or(0)}</span>
                                                                            }
                                                                                .into_any()
                                                                        } else {
                                                                            ().into_any()
                                                                        }}
                                                                    </td>
                                                                    <td class="px-6 py-4 text-gray-800 dark:text-gray-200 font-medium whitespace-nowrap">
                                                                        {e.completion_tokens.unwrap_or(0)}
                                                                    </td>
                                                                    <td class="px-6 py-4 text-gray-400 dark:text-gray-500 text-xs font-mono whitespace-nowrap">
                                                                        {e.client_ip.clone().unwrap_or_else(|| "-".to_string())}
                                                                    </td>
                                                                    <td class="px-6 py-4 text-center whitespace-nowrap">
                                                                        {{
                                                                            let code: u16 = e.status.parse().unwrap_or(0);
                                                                            let cls = if code >= 400 {
                                                                                "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400"
                                                                            } else {
                                                                                "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400"
                                                                            };
                                                                            view! {
                                                                                <span class=format!(
                                                                                    "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
                                                                                    cls,
                                                                                )>{e.status.clone()}</span>
                                                                            }
                                                                        }}
                                                                    </td>
                                                                </tr>
                                                            }
                                                        })
                                                        .collect::<Vec<_>>()}
                                                </tbody>
                                            </table>
                                        </div>
                                        <PaginationBar page total_signal=total per_page />
                                    </>
                                }
                                    .into_any()
                            }
                        }
                        Some(Err(ref e)) => {
                            view! {
                                <ErrorCard
                                    message=e.clone()
                                    on_retry=Box::new(move || rsc.refetch())
                                />
                            }
                                .into_any()
                        }
                        None => ().into_any(),
                    }}
                </Transition>
            </div>
        </div>

        {move || {
            detail_item
                .get()
                .map(|item| {
                    view! {
                        <ModalShell
                            on_close=move || detail_item.set(None)
                            title=ts!(LogDetail)
                            card_class="max-w-2xl"
                        >
                            <div class=CLASS_DETAIL_DIVIDER>
                                <DetailRow label=ts!(CreatedAt)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {timestamp_str(item.timestamp / 1_000_000)}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(Providers)>
                                    <span class=CLASS_DETAIL_VALUE>{item.provider_name}</span>
                                </DetailRow>
                                <DetailRow label=ts!(Models)>
                                    <span class=CLASS_DETAIL_VALUE>{item.model_name}</span>
                                </DetailRow>
                                <DetailRow label=ts!(LogEndpoint)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>{item.endpoint}</span>
                                </DetailRow>
                                <DetailRow label=ts!(LogUpstreamModel)>
                                    <span class=CLASS_DETAIL_VALUE>{item.upstream_model}</span>
                                </DetailRow>
                                <DetailRow label=ts!(LogProviderType)>
                                    <span class=CLASS_DETAIL_VALUE>{item.provider_type}</span>
                                </DetailRow>
                                <DetailRow label=ts!(LogApiKeyName)>
                                    <span class=CLASS_DETAIL_VALUE>
                                        {item.api_key_name.unwrap_or_default()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogStreaming)>
                                    <span class=CLASS_DETAIL_VALUE>
                                        {if item.is_streaming {
                                            ts!(LogStreamingY)
                                        } else {
                                            ts!(LogStreamingN)
                                        }}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogLatency)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {format!("{:.2}s", latency_s(item.latency_ms))}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogTtft)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item
                                            .time_to_first_token_ms
                                            .map(|v| format!("{:.2}s", latency_s(v)))
                                            .unwrap_or_default()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogInput)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item.prompt_tokens.unwrap_or(0).to_string()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogCachedTokens)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item.cached_tokens.unwrap_or(0).to_string()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogOutput)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item.completion_tokens.unwrap_or(0).to_string()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogTotalTokens)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item.total_tokens.unwrap_or(0).to_string()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogClientIp)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item.client_ip.clone().unwrap_or_else(|| "-".to_string())}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(LogResponseSize)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>
                                        {item
                                            .response_body_size
                                            .map(|v| format!("{} B", v))
                                            .unwrap_or_default()}
                                    </span>
                                </DetailRow>
                                <DetailRow label=ts!(TableStatus)>
                                    <span class=CLASS_DETAIL_VALUE_MONO>{item.status}</span>
                                </DetailRow>
                                {item
                                    .error_message
                                    .clone()
                                    .map(|msg| {
                                        view! {
                                            <DetailRow
                                                label=ts!(LogErrorMessage)
                                                value_class=CLASS_DETAIL_VALUE_PLAIN
                                            >
                                                <span class="text-sm text-right ml-4 text-red-600 dark:text-red-400 break-all max-w-[60%]">
                                                    {msg}
                                                </span>
                                            </DetailRow>
                                        }
                                    })}
                            </div>
                        </ModalShell>
                    }
                })
        }}
    }
}
