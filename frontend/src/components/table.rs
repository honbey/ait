use leptos::prelude::*;
use reactive_graph::traits::{IsDisposed, Write};
use reactive_stores::{Field, Patch, PatchField};

use crate::components::style::{
    CLASS_BG_MUTED, CLASS_BORDER_B, CLASS_BTN_PRIMARY, CLASS_CARD, CLASS_DETAIL_LABEL,
    CLASS_DETAIL_VALUE, CLASS_ICON_BTN, CLASS_PILL_GREEN, CLASS_PILL_RED, CLASS_TEXT_MUTED,
    CLASS_TOGGLE_LABEL,
};
use crate::{t, tr};

#[component]
pub fn DataTableCard(
    item_count: Signal<usize>,
    on_refresh: impl Fn() + 'static + Clone + Send,
    on_add: impl Fn() + 'static + Clone + Send,
    add_label: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=CLASS_CARD>
            <div class=format!("p-6 {} flex items-center justify-between", CLASS_BORDER_B)>
                <div class="flex items-center gap-3">
                    <span class=format!(
                        "text-sm {} {} px-3 py-1 rounded-full",
                        CLASS_TEXT_MUTED,
                        CLASS_BG_MUTED,
                    )>
                        {move || tr!(TotalCount, &[("count", &item_count.get().to_string())])()}
                    </span>
                    <button class=CLASS_ICON_BTN on:click=move |_| on_refresh()>
                        <i class="fas fa-sync-alt"></i>
                    </button>
                </div>
                <button
                    class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors flex items-center gap-2 text-sm font-medium cursor-pointer active:scale-95"
                    on:click=move |_| on_add()
                >
                    <i class="fas fa-plus"></i>
                    {add_label}
                </button>
            </div>
            {children()}
        </div>
    }
}

pub fn status_badge(enabled: bool) -> AnyView {
    if enabled {
        view! {
            <span class=format!(
                "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
                CLASS_PILL_GREEN,
            )>{t!(StatusEnabled)}</span>
        }
        .into_any()
    } else {
        view! {
            <span class=format!(
                "inline-block px-2 py-1 rounded-full text-xs font-medium {}",
                CLASS_PILL_RED,
            )>{t!(StatusDisabled)}</span>
        }
        .into_any()
    }
}

#[component]
pub fn DetailRow(
    label: String,
    children: Children,
    #[prop(optional, default = CLASS_DETAIL_VALUE)] value_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="flex justify-between py-2.5">
            <span class=CLASS_DETAIL_LABEL>{label}</span>
            <span class=value_class>{children()}</span>
        </div>
    }
}

#[component]
pub fn SubmitButton(is_edit: bool, #[prop(into)] pending: Signal<bool>) -> impl IntoView {
    view! {
        <button type="submit" disabled=move || pending.get() class=CLASS_BTN_PRIMARY>
            {move || {
                let label = if is_edit { t!(Save)() } else { t!(SaveCreate)() };
                if pending.get() {
                    view! {
                        <>
                            <i class="fas fa-spinner fa-spin"></i>
                            {label}
                        </>
                    }
                        .into_any()
                } else {
                    view! { {label} }.into_any()
                }
            }}
        </button>
    }
}

#[component]
pub fn ToggleField(id: &'static str, signal: RwSignal<bool>, label: String) -> impl IntoView {
    view! {
        <div class="flex items-center gap-2">
            <input
                id=id
                type="checkbox"
                prop:checked=signal
                on:input=move |ev| signal.set(event_target_checked(&ev))
            />
            <label for=id class=CLASS_TOGGLE_LABEL>
                {label}
            </label>
        </div>
    }
}

#[component]
pub fn DetailCloseButton(on_close: impl Fn() + 'static + Clone + Send) -> impl IntoView {
    view! {
        <div class="mt-6 flex justify-end">
            <button type="button" class=CLASS_BTN_PRIMARY on:click=move |_| on_close()>
                {t!(Close)}
            </button>
        </div>
    }
}

pub enum EntityModal<T: 'static> {
    Closed,
    Add,
    Detail(Field<T>),
    Edit(Field<T>),
    Delete(Field<T>),
}

impl<T: 'static> Clone for EntityModal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for EntityModal<T> {}

pub fn attach_save_effect<T, A>(
    action: &Action<A, Result<T, String>>,
    edit_field: Option<Field<T>>,
    store_items: impl Write<Value = Vec<T>> + 'static,
    form_error: RwSignal<String>,
    on_success: impl Fn() + 'static + Clone,
) where
    T: PatchField + Clone + 'static,
    A: 'static,
{
    let action = *action;
    let consumed = RwSignal::new(false);
    Effect::new(move |_| {
        if action.pending().get() {
            consumed.set(false);
            return;
        }
        if consumed.get_untracked() {
            return;
        }
        match action.value().get() {
            Some(Ok(model)) => {
                consumed.set(true);
                if let Some(field) = edit_field {
                    if !field.is_disposed() {
                        field.patch(model);
                        (on_success)();
                    }
                } else if let Some(mut items) = store_items.try_write() {
                    items.push(model);
                    (on_success)();
                }
            }
            Some(Err(e)) => {
                consumed.set(true);
                form_error.set(e);
            }
            None => {}
        }
    });
}

pub fn provider_display_name<'a>(id: &'a str, pairs: &'a [(String, String)]) -> &'a str {
    pairs
        .iter()
        .find(|(key, _)| key == id)
        .map(|(_, name)| name.as_str())
        .unwrap_or(id)
}

pub fn timestamp_str(ts: i64) -> String {
    if ts == 0 {
        return String::new();
    }
    let d = js_sys::Date::new(&((ts as f64) * 1000.0).into());
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}
