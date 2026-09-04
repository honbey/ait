use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

static NEXT_TOAST_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ToastLevel {
    Success,
    Error,
    Info,
}

fn toast_style(level: ToastLevel) -> (&'static str, &'static str, &'static str, &'static str) {
    match level {
        ToastLevel::Error => (
            "bg-red-50 dark:bg-red-900/30",
            "border-red-200 dark:border-red-700",
            "text-red-700 dark:text-red-300",
            "fa-exclamation-circle",
        ),
        ToastLevel::Success => (
            "bg-green-50 dark:bg-green-900/30",
            "border-green-200 dark:border-green-700",
            "text-green-700 dark:text-green-300",
            "fa-check-circle",
        ),
        ToastLevel::Info => (
            "bg-blue-50 dark:bg-blue-900/30",
            "border-blue-200 dark:border-blue-700",
            "text-blue-700 dark:text-blue-300",
            "fa-info-circle",
        ),
    }
}

#[derive(Clone)]
struct ToastData {
    id: u64,
    level: ToastLevel,
    message: String,
}

#[derive(Clone, Copy)]
pub struct ToastManager {
    toasts: RwSignal<Vec<ToastData>>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: RwSignal::new(Vec::new()),
        }
    }

    pub fn push(&self, level: ToastLevel, message: String) {
        let id = NEXT_TOAST_ID.fetch_add(1, Ordering::Relaxed);
        self.toasts
            .update(|t| t.push(ToastData { id, level, message }));
        let toasts = self.toasts;
        spawn_local(async move {
            TimeoutFuture::new(5000).await;
            toasts.update(|t| t.retain(|toast| toast.id != id));
        });
    }

    pub fn success(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Success, msg.into());
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Error, msg.into());
    }

    #[allow(dead_code)]
    pub fn info(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Info, msg.into());
    }

    pub fn remove(&self, id: u64) {
        self.toasts.update(|t| t.retain(|toast| toast.id != id));
    }
}

pub fn use_toast() -> ToastManager {
    use_context::<ToastManager>().expect("ToastManager not provided")
}

#[component]
pub fn ToastContainer() -> impl IntoView {
    let toast = use_toast();

    view! {
        <div class="fixed top-4 right-4 z-[100] flex flex-col gap-2 pointer-events-none">
            {move || {
                toast
                    .toasts
                    .get()
                    .into_iter()
                    .map(|data| {
                        let (bg, border, text, icon) = toast_style(data.level);
                        view! {
                            <div class=format!(
                                "flex items-center gap-2 px-4 py-3 rounded-lg \
                                    border shadow-lg {} {} {} pointer-events-auto",
                                bg,
                                border,
                                text,
                            )>
                                <i class=format!("fas {}", icon)></i>
                                <span class="text-sm font-medium">{data.message.clone()}</span>
                                <button
                                    class="ml-auto text-current opacity-50 \
                                    hover:opacity-100 cursor-pointer \
                                    transition-opacity active:scale-95"
                                    on:click=move |_| toast.remove(data.id)
                                >
                                    <i class="fas fa-times"></i>
                                </button>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}
