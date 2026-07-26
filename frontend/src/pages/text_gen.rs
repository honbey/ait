use futures_util::StreamExt;
use futures_util::stream::{Stream, try_unfold};
use js_sys::Uint8Array;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::api;
use crate::components::error_display::{ErrorCard, ErrorText};
use crate::components::style::{CLASS_INPUT, CLASS_LABEL, CLASS_TEXT_MUTED};
use crate::components::use_page_title;
use crate::{t, trs, ts};
use gloo_net::http::Request;

#[component]
pub fn TextGenPage() -> impl IntoView {
    use_page_title(&format!("Ait - {}", ts!(TextGeneration)));
    let models_resource = LocalResource::new(|| async move { api::fetch_models().await });

    let api_key: RwSignal<String> = RwSignal::new(String::new());
    let selected_model: RwSignal<String> = RwSignal::new(String::new());
    let prompt: RwSignal<String> = RwSignal::new(String::new());
    let temperature: RwSignal<String> = RwSignal::new("0.70".to_string());
    let max_tokens: RwSignal<String> = RwSignal::new("2048".to_string());
    let top_p: RwSignal<String> = RwSignal::new("0.90".to_string());
    let loading: RwSignal<bool> = RwSignal::new(false);
    let response: RwSignal<Option<String>> = RwSignal::new(None);
    let error: RwSignal<String> = RwSignal::new(String::new());
    let abort_controller: RwSignal<Option<web_sys::AbortController>> = RwSignal::new(None);

    on_cleanup(move || {
        if let Some(ctrl) = abort_controller.get_untracked() {
            ctrl.abort();
        }
    });

    let on_stop = move || {
        if let Some(ctrl) = abort_controller.get_untracked() {
            ctrl.abort();
        }
        loading.set(false);
        abort_controller.set(None);
    };

    let on_generate = move || {
        let key = api_key.get_untracked();
        if key.is_empty() {
            error.set(ts!(TextGenApiKeyRequired));
            return;
        }
        let model = selected_model.get_untracked();
        if model.is_empty() {
            error.set(ts!(Model));
            return;
        }

        let controller = web_sys::AbortController::new().expect("AbortController not available");
        let signal = controller.signal();
        abort_controller.set(Some(controller));

        loading.set(true);
        error.set(String::new());
        response.set(None);

        let prompt_text = prompt.get_untracked();
        let temp = temperature.get_untracked().parse::<f32>().ok();
        let mt = max_tokens.get_untracked().parse::<u32>().ok();
        let tp = top_p.get_untracked().parse::<f32>().ok();

        spawn_local(async move {
            match generate_completion_stream(
                &key,
                &model,
                &prompt_text,
                mt,
                temp,
                tp,
                Some(&signal),
            )
            .await
            {
                Ok(stream) => {
                    response.set(Some(String::new()));
                    let token_buf = Rc::new(RefCell::new(String::new()));
                    let flush_scheduled = Rc::new(RefCell::new(false));
                    let schedule_flush = {
                        let token_buf = Rc::clone(&token_buf);
                        let flush_scheduled = Rc::clone(&flush_scheduled);
                        move || {
                            if *flush_scheduled.borrow() {
                                return;
                            }
                            *flush_scheduled.borrow_mut() = true;
                            let _ = leptos_dom::helpers::request_animation_frame_with_handle({
                                let token_buf = Rc::clone(&token_buf);
                                let flush_scheduled = Rc::clone(&flush_scheduled);
                                move || {
                                    *flush_scheduled.borrow_mut() = false;
                                    let chunk = std::mem::take(&mut *token_buf.borrow_mut());
                                    if !chunk.is_empty() {
                                        response.update(|r| {
                                            if let Some(t) = r {
                                                t.push_str(&chunk);
                                            }
                                        });
                                    }
                                }
                            });
                        }
                    };
                    futures_util::pin_mut!(stream);
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(text) => {
                                token_buf.borrow_mut().push_str(&text);
                                schedule_flush();
                            }
                            Err(e) => {
                                if loading.get_untracked() {
                                    error.set(trs!(TextGenError, &[("msg", &e)]));
                                }
                                break;
                            }
                        }
                    }
                    let remaining = std::mem::take(&mut *token_buf.borrow_mut());
                    if !remaining.is_empty() {
                        response.update(|r| {
                            if let Some(t) = r {
                                t.push_str(&remaining);
                            }
                        });
                    }
                }
                Err(e) => {
                    if loading.get_untracked() {
                        error.set(trs!(TextGenError, &[("msg", &e)]));
                    }
                }
            }
            loading.set(false);
            abort_controller.set(None);
        });
    };

    Effect::new(move || {
        models_resource.with(|opt| {
            if let Some(Ok(models)) = opt
                && selected_model.with_untracked(|m| m.is_empty())
                && let Some(m) = models.iter().find(|m| m.enabled)
            {
                selected_model.set(m.name.clone());
            }
        });
    });

    let content = move || {
        models_resource.with(|opt| {
            match opt {
                None => ().into_any(),
                Some(Err(e)) => {
                    view! { <ErrorCard message=e.to_string() /> }.into_any()
                }
                Some(Ok(models)) => {
                    let opts: Vec<(String, String)> = models
                        .iter()
                        .filter(|m| m.enabled)
                        .map(|m| (m.name.clone(), m.name.clone()))
                        .collect();

                    let detail = view! {
                        <div class="flex gap-6">
                            <div class="w-1/3 flex flex-col gap-4">
                                <div>
                                    <label for="api-key-input" class=CLASS_LABEL>
                                        {t!(ApiKey)}
                                    </label>
                                    <input
                                        id="api-key-input"
                                        type="text"
                                        class=format!("{} font-mono", CLASS_INPUT)
                                        placeholder="sk-..."
                                        prop:value=api_key
                                        on:input=move |ev| api_key.set(event_target_value(&ev))
                                    />
                                </div>

                                <div>
                                    <label for="model-select" class=CLASS_LABEL>
                                        {t!(Model)}
                                    </label>
                                    <select
                                        id="model-select"
                                        class=CLASS_INPUT
                                        on:change=move |ev| {
                                            selected_model.set(event_target_value(&ev))
                                        }
                                    >
                                        {opts
                                            .iter()
                                            .cloned()
                                            .map(|(val, label)| {
                                                let v = val.clone();
                                                view! {
                                                    <option
                                                        value=v
                                                        selected=move || { selected_model.get() == val }
                                                    >
                                                        {label}
                                                    </option>
                                                }
                                            })
                                            .collect::<Vec<_>>()}
                                    </select>
                                </div>

                                <div>
                                    <label for="prompt-textarea" class=CLASS_LABEL>
                                        {t!(TextGenPrompt)}
                                    </label>
                                    <textarea
                                        id="prompt-textarea"
                                        class=format!("{} resize-y", CLASS_INPUT)
                                        placeholder=ts!(TextGenPromptPlaceholder)
                                        rows="8"
                                        prop:value=prompt
                                        on:input=move |ev| prompt.set(event_target_value(&ev))
                                    ></textarea>
                                </div>

                                <div class="space-y-3">
                                    <RangeField
                                        id="param-temperature"
                                        label=ts!(TextGenTemperature)
                                        min="0"
                                        max="2"
                                        step="0.01"
                                        value=temperature
                                    />
                                    <RangeField
                                        id="param-max-tokens"
                                        label=ts!(TextGenMaxTokens)
                                        min="1"
                                        max="8192"
                                        step="1"
                                        value=max_tokens
                                    />
                                    <RangeField
                                        id="param-top-p"
                                        label=ts!(TextGenTopP)
                                        min="0"
                                        max="1"
                                        step="0.01"
                                        value=top_p
                                    />
                                </div>

                                <button
                                    type="button"
                                    class="w-full py-3 bg-blue-500 hover:enabled:bg-blue-600 text-white font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 cursor-pointer active:scale-95"
                                    on:click=move |_| {
                                        if loading.get_untracked() {
                                            on_stop();
                                        } else {
                                            on_generate();
                                        }
                                    }
                                >
                                    <Show
                                        when=move || loading.get()
                                        fallback=move || view! { {t!(TextGenGenerate)} }
                                    >
                                        <i class="fas fa-stop"></i>
                                        {t!(TextGenStop)}
                                    </Show>
                                </button>

                                <ErrorText msg=error />
                            </div>

                            <div class="w-2/3 border-l border-gray-200 dark:border-gray-700 pl-6">
                                <h3 class=format!(
                                    "text-sm font-medium {} mb-2",
                                    CLASS_TEXT_MUTED,
                                )>{t!(TextGenResponse)}</h3>
                                <Show when=move || response.get().is_some()>
                                    <div class="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 text-gray-900 dark:text-gray-100 text-sm leading-relaxed whitespace-pre-wrap font-sans overflow-y-auto max-h-[70vh]">
                                        {move || response.get().unwrap_or_default()}
                                    </div>
                                </Show>
                            </div>
                        </div>
                    };
                    detail.into_any()
                }
            }
        })
    };

    view! { {content} }
}

#[component]
fn RangeField(
    id: &'static str,
    label: String,
    min: &'static str,
    max: &'static str,
    step: &'static str,
    value: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="flex items-center gap-2">
            <label for=id class="w-20 text-sm text-gray-600 dark:text-gray-400 shrink-0">
                {label}
            </label>
            <input
                type="range"
                id=id
                min=min
                max=max
                step=step
                class="flex-1 accent-indigo-600"
                prop:value=value
                on:input=move |ev| value.set(event_target_value(&ev))
            />
            <span class="w-14 text-right text-sm font-mono text-gray-700 dark:text-gray-300 shrink-0">
                {move || value.get()}
            </span>
        </div>
    }
}

// --- SSE streaming (was in api.rs) ---

async fn generate_completion_stream(
    token: &str,
    model: &str,
    prompt: &str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    signal: Option<&web_sys::AbortSignal>,
) -> Result<impl Stream<Item = Result<String, String>>, String> {
    let url = format!("{}/v1/completions", api::get_base_url());
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
    });
    if let Some(mt) = max_tokens {
        body["max_tokens"] = serde_json::json!(mt);
    }
    if let Some(t) = temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = top_p {
        body["top_p"] = serde_json::json!(p);
    }
    let auth = format!("Bearer {}", token);

    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", &auth)
        .abort_signal(signal)
        .body(body.to_string())
        .map_err(|e| format!("Failed to build request: {:?}", e))?
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {:?}", e))?;

    if !resp.ok() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&err_text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status));
        return Err(msg);
    }

    let raw_stream = resp.body().ok_or("Response has no body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader = raw_stream
        .get_reader()
        .dyn_into()
        .map_err(|_| "Failed to create reader".to_string())?;

    Ok(sse_stream_from_reader(reader))
}

fn sse_stream_from_reader(
    reader: web_sys::ReadableStreamDefaultReader,
) -> impl Stream<Item = Result<String, String>> {
    try_unfold((reader, Vec::<u8>::new()), |(reader, mut buf)| async move {
        loop {
            if let Some(result) = poll_sse(&mut buf) {
                match result {
                    SsePoll::Token(t) => return Ok::<_, String>(Some((t, (reader, buf)))),
                    SsePoll::End => return Ok(None),
                    SsePoll::Skip => continue,
                }
            }

            let promise = reader.read();
            let result = JsFuture::from(promise)
                .await
                .map_err(|e| format!("Stream read error: {:?}", e))?;

            let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if done {
                if !buf.is_empty() {
                    let remaining = String::from_utf8_lossy(&buf).to_string();
                    buf.clear();
                    return Ok::<_, String>(Some((remaining, (reader, buf))));
                }
                return Ok(None);
            }

            if let Ok(value) = js_sys::Reflect::get(&result, &JsValue::from_str("value")) {
                let array = Uint8Array::new(&value);
                buf.extend_from_slice(&array.to_vec());
            }
        }
    })
}

enum SsePoll {
    Token(String),
    End,
    Skip,
}

fn poll_sse(buf: &mut Vec<u8>) -> Option<SsePoll> {
    let (boundary, n) = buf
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|p| (p, 2))
        .or_else(|| {
            buf.windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| (p, 4))
        })?;

    let event_bytes: Vec<u8> = buf.drain(..boundary).collect();
    buf.drain(..n);

    let event_str = String::from_utf8_lossy(&event_bytes);
    for line in event_str.lines() {
        let payload = match line.strip_prefix("data: ") {
            Some(p) => p.trim(),
            None => continue,
        };
        if payload == "[DONE]" {
            return Some(SsePoll::End);
        }
        if let Some(token) = parse_token(payload) {
            return Some(SsePoll::Token(token));
        }
    }
    Some(SsePoll::Skip)
}

fn parse_token(payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;

    if let Some(content) = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        && !content.is_empty()
    {
        return Some(content.to_string());
    }

    if let Some(text) = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        && !text.is_empty()
    {
        return Some(text.to_string());
    }

    if let Some(response) = json.get("response").and_then(|r| r.as_str())
        && !response.is_empty()
    {
        return Some(response.to_string());
    }

    None
}
