use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore::web::{Keyed, KeyedProps};

#[derive(Clone, Copy, PartialEq)]
pub enum ToastLevel {
    Error,
    Success,
    Info,
}

#[derive(Clone, PartialEq)]
pub struct ToastData {
    id: u64,
    level: ToastLevel,
    message: String,
}

#[derive(Clone)]
pub struct ToastManager {
    toasts: Signal<Vec<ToastData>>,
    next_id: Rc<Cell<u64>>,
    timers: Rc<RefCell<HashMap<u64, Timeout>>>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: create_signal(Vec::new()),
            next_id: Rc::new(Cell::new(0)),
            timers: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn push(&self, level: ToastLevel, message: String) {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.toasts
            .update(|t| t.push(ToastData { id, level, message }));
        let manager = self.clone();
        let timer = Timeout::new(5000, move || {
            manager.toasts.update(|t| t.retain(|toast| toast.id != id));
            manager.timers.borrow_mut().remove(&id);
        });
        self.timers.borrow_mut().insert(id, timer);
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Error, msg.into());
    }

    pub fn success(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Success, msg.into());
    }

    #[allow(dead_code)]
    pub fn info(&self, msg: impl Into<String>) {
        self.push(ToastLevel::Info, msg.into());
    }

    pub fn remove(&self, id: u64) {
        self.toasts.update(|t| t.retain(|toast| toast.id != id));
        self.timers.borrow_mut().remove(&id);
    }
}

pub fn render_toast_container() -> View {
    let manager = use_context::<ToastManager>();
    div()
        .class("fixed top-4 right-4 z-[100] flex flex-col gap-2")
        .children(Keyed(
            KeyedProps::builder()
                .list(manager.toasts)
                .view(move |data| render_single_toast(data, manager.clone()))
                .key(|data| data.id)
                .build(),
        ))
        .into()
}

fn render_single_toast(data: ToastData, manager: ToastManager) -> View {
    let (bg, border, text, icon) = match data.level {
        ToastLevel::Error => (
            "bg-red-50 dark:bg-red-900/30",
            "border-red-200 dark:border-red-700",
            "text-red-700 dark:text-red-300",
            "fas fa-exclamation-circle",
        ),
        ToastLevel::Success => (
            "bg-green-50 dark:bg-green-900/30",
            "border-green-200 dark:border-green-700",
            "text-green-700 dark:text-green-300",
            "fas fa-check-circle",
        ),
        ToastLevel::Info => (
            "bg-blue-50 dark:bg-blue-900/30",
            "border-blue-200 dark:border-blue-700",
            "text-blue-700 dark:text-blue-300",
            "fas fa-info-circle",
        ),
    };
    let id = data.id;
    let msg = data.message;
    div()
        .class(format!(
            "flex items-center gap-3 px-4 py-3 rounded-lg border shadow-lg {} {} {}",
            bg, border, text
        ))
        .children((
            i().class(icon),
            span().class("text-sm font-medium").children(msg),
            button()
                .class(
                    "ml-auto text-current opacity-50 hover:opacity-100 cursor-pointer transition-opacity",
                )
                .on(events::click, move |_| manager.remove(id))
                .children(i().class("fas fa-times")),
        ))
        .into()
}
