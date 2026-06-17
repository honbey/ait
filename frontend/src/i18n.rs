use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sycamore::prelude::*;
use sycamore::web::console_error;
use sycamore_futures::spawn_local_scoped;

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
        let this = Self(Rc::new(inner));
        this.fetch_and_cache(initial_lang);
        this
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
            self.fetch_and_cache(lang);
        }
    }

    pub fn t(&self, key: &str) -> String {
        self.0
            .translations
            .with(|map| map.get(key).cloned().unwrap_or_else(|| key.to_string()))
    }

    pub fn t_replace(&self, key: &str, placeholder: &str, value: &str) -> String {
        self.t(key)
            .replace(&format!("{{{{ {} }}}}", placeholder), value)
    }

    fn fetch_and_cache(&self, lang: &str) {
        let this = self.clone();
        let lang = lang.to_string();
        spawn_local_scoped(async move {
            match Self::fetch_translations(&lang).await {
                Ok(map) => {
                    this.0.cache.borrow_mut().insert(lang.clone(), map.clone());
                    if this.0.lang.get_clone() == lang {
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
