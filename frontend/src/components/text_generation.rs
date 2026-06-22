use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::api::generate_completion;
use crate::components::modal::{form_field, select_input};
use crate::i18n::{I18n, K};
use crate::models::Model;

#[derive(Props)]
pub struct TextGenerationProps {
    pub models: Vec<Model>,
}

#[component]
pub fn TextGeneration(props: TextGenerationProps) -> View {
    let i18n = use_context::<I18n>();
    let opts: Vec<(String, String)> = props
        .models
        .iter()
        .filter(|m| m.enabled)
        .map(|m| {
            let n = m.name.clone();
            (n.clone(), n)
        })
        .collect();

    let api_key = create_signal(String::new());
    let selected_model = create_signal(
        props
            .models
            .iter()
            .find(|m| m.enabled)
            .map(|m| m.name.clone())
            .unwrap_or_default(),
    );
    let prompt = create_signal(String::new());
    let temperature = create_signal("0.70".to_string());
    let max_tokens = create_signal("2048".to_string());
    let top_p = create_signal("0.90".to_string());
    let loading = create_signal(false);
    let response = create_signal::<Option<String>>(None);
    let error = create_signal(String::new());

    let on_generate = {
        let i18n_gen = i18n.clone();
        move |_| {
            let i18n = i18n_gen.clone();
            spawn_local_scoped(async move {
                let key = api_key.get_clone();
                if key.is_empty() {
                    error.set("API Key is required".into());
                    return;
                }
                let model = selected_model.get_clone();
                if model.is_empty() {
                    error.set(i18n.t(K::TextGenSelectModel));
                    return;
                }
                loading.set(true);
                error.set(String::new());
                response.set(None);
                let prompt_text = prompt.get_clone();
                let temp = temperature.get_clone().parse::<f32>().ok();
                let mt = max_tokens.get_clone().parse::<u32>().ok();
                let tp = top_p.get_clone().parse::<f32>().ok();
                match generate_completion(&key, &model, &prompt_text, mt, temp, tp).await {
                    Ok(text) => response.set(Some(text)),
                    Err(e) => error.set(i18n.t_replace(K::TextGenError, "msg", &e.to_string())),
                }
                loading.set(false);
            });
        }
    };

    div()
        .children(
            div()
                .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 max-w-6xl mx-auto")
                .children(
                    div()
                        .class("flex gap-6")
                        .children((
                            // Left column: form (1/3)
                            div()
                                .class("w-1/3 flex flex-col gap-4")
                                .children((
                                    form_field(
                                        "text-gen-api-key".into(),
                                        i18n.t(K::TextGenApiKey),
                                        input()
                                            .attr("id", "text-gen-api-key")
                                            .attr("type", "text")
                                            .attr("placeholder", "sk-...")
                                            .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none font-mono")
                                            .bind(bind::value, api_key)
                                            .into(),
                                    ),
                                    form_field(
                                        "text-gen-model".into(),
                                        i18n.t(K::TextGenSelectModel),
                                        select_input("text-gen-model".into(), selected_model, opts),
                                    ),
                                    form_field(
                                        "text-gen-prompt".into(),
                                        i18n.t(K::TextGenPrompt),
                                        textarea()
                                            .attr("id", "text-gen-prompt")
                                            .attr("placeholder", i18n.t(K::TextGenPromptPlaceholder))
                                            .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none resize-y")
                                            .attr("rows", "8")
                                            .bind(bind::value, prompt)
                                            .into(),
                                    ),
                                    // Parameters
                                    div()
                                        .class("space-y-3")
                                        .children((
                                            range_field("text-gen-temperature".into(), i18n.t(K::TextGenTemperature), "0", "2", "0.01", temperature),
                                            range_field("text-gen-max-tokens".into(), i18n.t(K::TextGenMaxTokens), "1", "8192", "1", max_tokens),
                                            range_field("text-gen-top-p".into(), i18n.t(K::TextGenTopP), "0", "1", "0.01", top_p),
                                        )),
                                    // Generate button
                                    button()
                                        .attr("type", "button")
                                        .disabled(move || loading.get())
                                        .class("w-full py-3 bg-blue-500 hover:enabled:bg-blue-600 text-white font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2")
                                        .on(events::click, on_generate)
                                        .children(View::from_dynamic({
                                            let i18n = i18n.clone();
                                            move || -> View {
                                                if loading.get() {
                                                    div().class("flex items-center gap-2").children((
                                                        i().class("fas fa-spinner animate-spin"),
                                                        span().children(i18n.t(K::TextGenGenerating)),
                                                    )).into()
                                                } else {
                                                    span().children(i18n.t(K::TextGenGenerate)).into()
                                                }
                                            }
                                        })),
                                    // Error
                                    View::from_dynamic(move || {
                                        let msg = error.get_clone();
                                        if msg.is_empty() {
                                            View::new()
                                        } else {
                                            div()
                                                .class("p-4 bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 rounded-lg text-sm")
                                                .children(msg)
                                                .into()
                                        }
                                    }),
                                )),
                            // Right column: output (2/3)
                            div()
                                .class("w-2/3 border-l border-gray-200 dark:border-gray-700 pl-6")
                                .children(
                                    div()
                                        .class("sticky top-20")
                                        .children(
                                            div()
                                                .class("space-y-2")
                                                .children((
                                                    h3()
                                                        .class("text-sm font-medium text-gray-500 dark:text-gray-400")
                                                        .children(i18n.t(K::TextGenResponse)),
                                                    View::from_dynamic(move || -> View {
                                                        if let Some(text) = response.get_clone() {
                                                            div()
                                                                .class("p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 text-gray-900 dark:text-gray-100 text-sm leading-relaxed whitespace-pre-wrap font-sans overflow-y-auto max-h-[70vh]")
                                                                .children(text)
                                                                .into()
                                                        } else {
                                                            View::new()
                                                        }
                                                    }),
                                                )),
                                        ),
                                ),
                        )),
                ),
        )
        .into()
}

fn range_field(
    id: String,
    label_text: String,
    min: &'static str,
    max: &'static str,
    step: &'static str,
    value: Signal<String>,
) -> View {
    div()
        .class("flex items-center gap-3")
        .children((
            label()
                .attr("for", id.clone())
                .class("w-20 text-sm text-gray-600 dark:text-gray-400 shrink-0")
                .children(label_text),
            input()
                .attr("id", id)
                .attr("type", "range")
                .attr("min", min)
                .attr("max", max)
                .attr("step", step)
                .class("flex-1 accent-indigo-600")
                .bind(bind::value, value),
            View::from_dynamic(move || -> View {
                span()
                    .class("w-14 text-right text-sm font-mono text-gray-700 dark:text-gray-300 shrink-0")
                    .children(value.get_clone())
                    .into()
            }),
        ))
        .into()
}
