use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

thread_local! {
    static ECHARTS_READY: RefCell<Option<js_sys::Promise>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct Chart(JsValue);

impl Chart {
    /// Initialise (or reuse) the ECharts instance on `dom`. Returns `None`
    /// when the injected `echarts` global is missing or malformed — the chart
    /// simply does not render instead of panicking the page.
    pub fn init(dom: &web_sys::HtmlElement) -> Option<Self> {
        let echarts = js_sys::Reflect::get(&js_sys::global(), &"echarts".into()).ok()?;

        let get_instance: js_sys::Function = js_sys::Reflect::get(&echarts, &"getInstanceByDom".into())
            .ok()?
            .dyn_into()
            .ok()?;
        if let Some(existing) = get_instance
            .call1(&JsValue::UNDEFINED, dom)
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
        {
            return Some(Chart(existing));
        }

        let init: js_sys::Function = js_sys::Reflect::get(&echarts, &"init".into())
            .ok()?
            .dyn_into()
            .ok()?;
        Some(Chart(init.call1(&JsValue::UNDEFINED, dom).ok()?))
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

    pub fn resize(&self) {
        let resize: js_sys::Function = js_sys::Reflect::get(&self.0, &"resize".into())
            .expect("chart.resize not found")
            .unchecked_into();
        let _ = resize.call0(&self.0);
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
                    let onload_s = s.clone();
                    let onload = Closure::once(move || {
                        // Clear handlers so the script element stops
                        // referencing the closures, letting GC reclaim them.
                        onload_s.set_onload(None);
                        onload_s.set_onerror(None);
                        let _ = resolve.call0(&JsValue::UNDEFINED);
                    });
                    s.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();

                    let onerror_s = s.clone();
                    let onerror = Closure::once(move || {
                        onerror_s.set_onload(None);
                        onerror_s.set_onerror(None);
                        let _ = reject.call0(&JsValue::UNDEFINED);
                    });
                    s.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                    onerror.forget();

                    let _ = document.head().unwrap().append_child(s.as_ref());
                })
            })
            .clone()
    });

    let ok = JsFuture::from(promise).await.is_ok();
    if !ok {
        ECHARTS_READY.with(|cell| *cell.borrow_mut() = None);
    }
    ok
}

pub fn use_chart(node: NodeRef<leptos::html::Div>) -> RwSignal<Option<Chart>> {
    let chart: RwSignal<Option<Chart>> = RwSignal::new(None);
    let loading = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let observer = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    // Holds the ResizeObserver callback closure so it can be dropped on
    // cleanup; without this the forget()'d closure leaks permanently.
    let observer_cb = StoredValue::new_local(
        None::<Closure<dyn FnMut(Vec<web_sys::ResizeObserverEntry>, web_sys::ResizeObserver)>>,
    );

    Effect::new({
        let loading = loading.clone();
        let alive = alive.clone();

        move |_| {
            let Some(el) = node.get() else { return };
            if observer.get_value().is_some() {
                return;
            }
            if loading.load(Ordering::Relaxed) {
                return;
            }

            let loading_cb = loading.clone();
            let alive_cb = alive.clone();
            let cb_el = el.clone();
            let chart_cb = chart;

            let cb = Closure::<dyn FnMut(Vec<web_sys::ResizeObserverEntry>, web_sys::ResizeObserver)>::new(
                move |_entries: Vec<web_sys::ResizeObserverEntry>, _observer: web_sys::ResizeObserver| {
                    if cb_el.offset_width() == 0 {
                        return;
                    }

                    if let Some(c) = chart_cb.get_untracked() {
                        c.resize();
                    } else if !loading_cb.load(Ordering::Relaxed) {
                        loading_cb.store(true, Ordering::Relaxed);
                        spawn_local({
                            let cb_el = cb_el.clone();
                            let loading_cb = loading_cb.clone();
                            let alive_cb = alive_cb.clone();
                            async move {
                                let loaded = ensure_echarts_loaded().await;
                                if loaded {
                                    if alive_cb.load(Ordering::Relaxed) {
                                        if let Some(c) = Chart::init(&cb_el) {
                                            chart_cb.set(Some(c));
                                        }
                                    } else if let Some(c) = Chart::init(&cb_el) {
                                        c.dispose();
                                    }
                                } else {
                                    loading_cb.store(false, Ordering::Relaxed);
                                }
                            }
                        });
                    }
                },
            );

            let f: &js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>();
            let ro = web_sys::ResizeObserver::new(f).expect("ResizeObserver::new failed");
            let el_ref: &web_sys::Element = el.as_ref();
            ro.observe(el_ref);
            observer.set_value(Some(ro));
            observer_cb.set_value(Some(cb));

            if el.offset_width() > 0
                && chart.get_untracked().is_none()
                && !loading.load(Ordering::Relaxed)
            {
                loading.store(true, Ordering::Relaxed);
                let loading = loading.clone();
                let alive = alive.clone();
                let el_init = el.clone();
                spawn_local(async move {
                    let loaded = ensure_echarts_loaded().await;
                    if loaded {
                        if alive.load(Ordering::Relaxed) {
                            if let Some(c) = Chart::init(&el_init) {
                                chart.set(Some(c));
                            }
                        } else if let Some(c) = Chart::init(&el_init) {
                            c.dispose();
                        }
                    } else {
                        loading.store(false, Ordering::Relaxed);
                    }
                });
            }
        }
    });

    on_cleanup(move || {
        alive.store(false, Ordering::Relaxed);
        if let Some(ro) = observer.get_value() {
            ro.disconnect();
        }
        // Drop the callback closure to release its JS reference; the
        // observer no longer references it after disconnect.
        observer_cb.set_value(None);
        if let Some(c) = chart.get_untracked() {
            c.dispose();
        }
    });

    chart
}
