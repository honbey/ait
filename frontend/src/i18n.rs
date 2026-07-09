use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gloo_net::Error as NetError;
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

include!(concat!(env!("OUT_DIR"), "/i18n_keys.rs"));

struct I18nInner {
    lang: RwSignal<String>,
    translations: RwSignal<HashMap<String, String>>,
    cache: RwLock<HashMap<String, HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct I18n(Arc<I18nInner>);

impl I18n {
    pub fn new(initial_lang: &str) -> Self {
        let default = Self::embedded_translations(initial_lang);
        let inner = I18nInner {
            lang: RwSignal::new(initial_lang.to_string()),
            translations: RwSignal::new(default.clone()),
            cache: RwLock::new(HashMap::from([(initial_lang.to_string(), default)])),
        };
        Self(Arc::new(inner))
    }

    pub fn lang(&self) -> String {
        self.0.lang.get()
    }

    pub fn lang_untracked(&self) -> String {
        self.0.lang.get_untracked()
    }

    pub fn set_lang(&self, lang: &str) {
        if lang == self.0.lang.get_untracked() {
            return;
        }
        self.0.lang.set(lang.to_string());
        if let Some(cached) = self.0.cache.read().unwrap().get(lang).cloned() {
            self.0.translations.set(cached);
        } else {
            let embedded = Self::embedded_translations(lang);
            self.0
                .cache
                .write()
                .unwrap()
                .insert(lang.to_string(), embedded.clone());
            self.0.translations.set(embedded);
            self.fetch_and_cache(lang);
        }
    }

    pub fn t(&self, key: K) -> String {
        let k = key.as_str();
        self.0
            .translations
            .with(|map| map.get(k).cloned().unwrap_or_else(|| k.to_string()))
    }

    /// Non-reactive version of `t`
    pub fn t_untracked(&self, key: K) -> String {
        let k = key.as_str();
        self.0
            .translations
            .with_untracked(|map| map.get(k).cloned().unwrap_or_else(|| k.to_string()))
    }

    /// Non-reactive `t_replace` (placeholder substitution)
    pub fn t_replace_untracked(&self, key: K, pairs: &[(&str, &str)]) -> String {
        pairs.iter().fold(self.t_untracked(key), |s, (ph, val)| {
            s.replace(&format!("{{{{ {} }}}}", ph), val)
        })
    }

    /// Reactive `t_replace` (placeholder substitution)
    pub fn t_replace(&self, key: K, pairs: &[(&str, &str)]) -> String {
        pairs.iter().fold(self.t(key), |s, (ph, val)| {
            s.replace(&format!("{{{{ {} }}}}", ph), val)
        })
    }

    // Multiple fetch_and_cache calls for the same language are not deduplicated,
    // leading to redundant network requests.
    // Only the latest language's fetch will update the translations.
    fn fetch_and_cache(&self, lang: &str) {
        let this = self.clone();
        let lang = lang.to_string();
        spawn_local(async move {
            match Self::fetch_translations(&lang).await {
                Ok(map) => {
                    this.0
                        .cache
                        .write()
                        .unwrap()
                        .insert(lang.clone(), map.clone());
                    if this.0.lang.get_untracked() == lang {
                        this.0.translations.set(map);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load locale {}: {:?}", lang, e);
                }
            }
        });
    }

    async fn fetch_translations(lang: &str) -> Result<HashMap<String, String>, NetError> {
        let url = format!("/locales/{}.json", lang);
        let resp = Request::get(&url).send().await?;
        if !resp.ok() {
            return Err(NetError::GlooError(format!(
                "HTTP {} loading locale {}",
                resp.status(),
                lang
            )));
        }
        resp.json().await
    }

    fn embedded_translations(lang: &str) -> HashMap<String, String> {
        let json = match lang {
            "zh" => include_str!("../locales/zh.json"),
            "en" => include_str!("../locales/en.json"),
            _ => include_str!("../locales/zh.json"),
        };
        serde_json::from_str(json).unwrap_or_default()
    }
}

pub fn use_i18n() -> I18n {
    use_context::<I18n>().expect("I18n")
}

/// Reactive: returns `move || String` for use in `view!`. Subscribes to signals,
/// re-renders on language switch.
#[macro_export]
macro_rules! t {
    ($key:ident) => {
        move || $crate::i18n::use_i18n().t($crate::i18n::K::$key)
    };
}

/// Static: returns `String` directly, does NOT subscribe to signals.
/// Use only in Effect / event callbacks or other non-reactive contexts.
#[macro_export]
macro_rules! ts {
    ($key:ident) => {
        $crate::i18n::use_i18n().t_untracked($crate::i18n::K::$key)
    };
}

/// Reactive: `t!` + placeholder substitution. Subscribe to signals, re-renders on language switch.
/// Use directly as `{tr!(Key, &[...])}` in `view!`.
#[macro_export]
macro_rules! tr {
    ($key:ident, $repl:expr) => {
        move || $crate::i18n::use_i18n().t_replace($crate::i18n::K::$key, $repl)
    };
}

/// Static: `ts!` + placeholder substitution. Does NOT subscribe to signals.
/// Use only in Effect / event callbacks or other non-reactive contexts.
#[macro_export]
macro_rules! trs {
    ($key:ident, $repl:expr) => {
        $crate::i18n::use_i18n().t_replace_untracked($crate::i18n::K::$key, $repl)
    };
}
