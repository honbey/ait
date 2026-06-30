use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sycamore::prelude::*;
use sycamore::web::console_error;
use sycamore_futures::spawn_local_scoped;

include!(concat!(env!("OUT_DIR"), "/i18n_keys.rs"));

struct I18nInner {
    lang: Signal<String>,
    translations: Signal<HashMap<String, String>>,
    cache: RefCell<HashMap<String, HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct I18n(Rc<I18nInner>);

impl I18n {
    pub fn new(initial_lang: &str) -> Self {
        let default = Self::embedded_translations(initial_lang);
        let inner = I18nInner {
            lang: create_signal(initial_lang.to_string()),
            translations: create_signal(default.clone()),
            cache: RefCell::new(HashMap::from([(initial_lang.to_string(), default)])),
        };
        Self(Rc::new(inner))
        // No fetch for initial language — embedded translations are loaded directly.
        // set_lang handles non-initial languages with embedded fallback + lazy fetch.
    }

    pub fn lang(&self) -> String {
        self.0.lang.get_clone()
    }

    pub fn set_lang(&self, lang: &str) {
        if lang == &*self.0.lang.get_clone() {
            return;
        }
        self.0.lang.set(lang.to_string());
        if let Some(cached) = self.0.cache.borrow().get(lang).cloned() {
            self.0.translations.set(cached);
        } else {
            // Use embedded translations as immediate fallback while fetch happens
            let embedded = Self::embedded_translations(lang);
            self.0
                .cache
                .borrow_mut()
                .insert(lang.to_string(), embedded.clone());
            self.0.translations.set(embedded);
            // Async fetch can overwrite with updated translations at runtime
            self.fetch_and_cache(lang);
        }
    }

    pub fn t(&self, key: K) -> String {
        let k = key.as_str();
        self.0
            .translations
            .with(|map| map.get(k).cloned().unwrap_or_else(|| k.to_string()))
    }

    pub fn t_replace(&self, key: K, placeholder: &str, value: &str) -> String {
        self.t(key)
            .replace(&format!("{{{{ {} }}}}", placeholder), value)
    }

    fn fetch_and_cache(&self, lang: &str) {
        let this = self.clone();
        let lang = lang.to_string();
        spawn_local_scoped(async move {
            match Self::fetch_translations(&lang).await {
                Ok(map) => {
                    let is_current = this.0.lang.get_clone() == lang;
                    this.0.cache.borrow_mut().insert(lang, map.clone());
                    if is_current {
                        this.0.translations.set(map);
                    }
                }
                Err(e) => {
                    console_error!("Failed to load locale {}: {:?}", lang, e);
                }
            }
        });
    }

    async fn fetch_translations(lang: &str) -> Result<HashMap<String, String>, gloo_net::Error> {
        let url = format!("/locales/{}.json", lang);
        let resp = gloo_net::http::Request::get(&url).send().await?;
        if !resp.ok() {
            return Err(gloo_net::Error::GlooError(format!(
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
