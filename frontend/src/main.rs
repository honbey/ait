mod api;
mod app;
mod auth;
mod components;
mod i18n;
mod pages;
mod storage;
mod time_utils;

use app::*;
use leptos::{logging, mount};

pub fn main() {
    console_error_panic_hook::set_once();
    logging::log!("Ait - Unified management of LLM providers, models, and API keys");
    mount::mount_to_body(App);
}
