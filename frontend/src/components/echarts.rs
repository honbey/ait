use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

thread_local! {
    static ECHARTS_READY: RefCell<Option<js_sys::Promise>> = const { RefCell::new(None) };
    static CHART_CACHE: RefCell<HashMap<&'static str, Chart>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct Chart(JsValue);

impl Chart {
    pub fn new(dom: &web_sys::HtmlElement) -> Self {
        let echarts =
            js_sys::Reflect::get(&js_sys::global(), &"echarts".into()).expect("echarts not found");
        let init: js_sys::Function = js_sys::Reflect::get(&echarts, &"init".into())
            .expect("echarts.init not found")
            .unchecked_into();
        let result = init
            .call1(&JsValue::UNDEFINED, dom)
            .expect("echarts.init failed");
        Chart(result)
    }

    pub fn set_option(&self, option: &JsValue) {
        let set_option: js_sys::Function = js_sys::Reflect::get(&self.0, &"setOption".into())
            .expect("chart.setOption not found")
            .unchecked_into();
        let _ = set_option.call1(&self.0, option);
    }

    pub fn dispose(&self) {
        let dispose: js_sys::Function = js_sys::Reflect::get(&self.0, &"dispose".into())
            .expect("chart.dispose not found")
            .unchecked_into();
        let _ = dispose.call0(&self.0);
    }
}

async fn ensure_echarts_loaded() -> bool {
    if js_sys::Reflect::has(&js_sys::global(), &"echarts".into()).unwrap_or(false) {
        return true;
    }

    let promise = ECHARTS_READY.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                let document = web_sys::window().unwrap().document().unwrap();
                let s: web_sys::HtmlScriptElement = document
                    .create_element("script")
                    .unwrap()
                    .dyn_into()
                    .expect("expected HtmlScriptElement");
                s.set_src("/echarts.min.js");

                js_sys::Promise::new(&mut |resolve: js_sys::Function, reject: js_sys::Function| {
                    let onload = Closure::once(move || {
                        let _ = resolve.call0(&JsValue::UNDEFINED);
                    });
                    s.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();

                    let onerror = Closure::once(move || {
                        let _ = reject.call0(&JsValue::UNDEFINED);
                    });
                    s.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                    onerror.forget();

                    let _ = document.head().unwrap().append_child(s.as_ref());
                })
            })
            .clone()
    });

    JsFuture::from(promise).await.is_ok()
}

fn get_chart_element(id: &str) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

pub fn use_echarts(id: &'static str) -> (RwSignal<Option<Chart>>, Arc<AtomicBool>) {
    let chart: RwSignal<Option<Chart>> = RwSignal::new(None);
    let mounted = Arc::new(AtomicBool::new(true));

    // Restore cached chart if available (component re-render path)
    if let Some(cached) = CHART_CACHE.with(|c| c.borrow().get(id).cloned()) {
        chart.set(Some(cached));
    }

    on_cleanup({
        let mounted = mounted.clone();
        move || {
            mounted.store(false, Ordering::Relaxed);
            if let Some(c) = chart.get_untracked() {
                c.dispose();
            }
            CHART_CACHE.with(|c| c.borrow_mut().remove(id));
        }
    });

    (chart, mounted)
}

pub fn init_or_show_chart(
    id: &'static str,
    chart: RwSignal<Option<Chart>>,
    mounted: Arc<AtomicBool>,
    option: JsValue,
) {
    // Fast path: chart already cached — sync setOption
    if let Some(cached) = CHART_CACHE.with(|c| c.borrow().get(id).cloned()) {
        cached.set_option(&option);
        chart.set(Some(cached));
        return;
    }

    // Slow path: first init
    spawn_local(async move {
        if !ensure_echarts_loaded().await || !mounted.load(Ordering::Relaxed) {
            return;
        }
        if let Some(el) = get_chart_element(id) {
            let c = Chart::new(&el);
            c.set_option(&option);
            if mounted.load(Ordering::Relaxed) {
                chart.set(Some(c.clone()));
                CHART_CACHE.with(|cache| cache.borrow_mut().insert(id, c));
            } else {
                c.dispose();
            }
        }
    });
}
