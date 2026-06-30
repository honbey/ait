use sycamore::prelude::*;
use sycamore_futures::spawn_local_scoped;
use sycamore_router::navigate;

use crate::components::auth_form::{
    auth_error_display, auth_input_field, auth_link_footer, auth_page_shell, auth_submit_button,
};
use crate::components::modal::FormStatus;
use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};

pub fn render_register_view() -> View {
    let i18n = use_context::<I18n>();
    let toast = use_context::<ToastManager>();
    let username = create_signal(String::new());
    let password = create_signal(String::new());
    let registration_code = create_signal(String::new());
    let FormStatus {
        err: error,
        loading,
    } = FormStatus::new();

    let i18n_submit = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        if loading.get() {
            return;
        }

        if username.get_clone().is_empty() || password.get_clone().is_empty() {
            error.set(i18n_submit.t(K::RegisterRequired));
            return;
        }

        loading.set(true);
        let u = username.get_clone();
        let p = password.get_clone();
        let c = registration_code.get_clone();
        let i18n_async = i18n_submit.clone();
        let toast = toast.clone();
        spawn_local_scoped(async move {
            match crate::api::register_api(&u, &p, &c).await {
                Ok(()) => {
                    toast.success(i18n_async.t(K::RegistrationSuccessful));
                    navigate("/login");
                }
                Err(e) => {
                    error.set(i18n_async.t_replace(K::RegisterError, "msg", &e.to_string()));
                    loading.set(false);
                }
            }
        });
    };

    auth_page_shell(
        on_submit,
        i18n.t(K::Register),
        (
            auth_input_field(
                i18n.t(K::Username),
                "text",
                i18n.t(K::Username),
                username,
                error,
                false,
            ),
            auth_input_field(
                i18n.t(K::Password),
                "password",
                i18n.t(K::Password),
                password,
                error,
                false,
            ),
            auth_input_field(
                i18n.t(K::RegistrationCode),
                "text",
                i18n.t(K::RegistrationCode),
                registration_code,
                error,
                true,
            ),
            auth_error_display(error),
            auth_submit_button(loading, i18n.t(K::RegisterBtn)),
            auth_link_footer(i18n.t(K::HaveAccountLogin), i18n.t(K::Login), move |_| {
                navigate("/login")
            }),
        ),
    )
}
