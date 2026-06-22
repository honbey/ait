use sycamore::prelude::*;
use sycamore_futures::spawn_local_scoped;

use crate::components::auth_form::{
    auth_error_display, auth_input_field, auth_link_footer, auth_page_shell, auth_submit_button,
};
use crate::i18n::{I18n, K};
use crate::route::Route;

pub fn render_login_view(
    authenticated: Signal<bool>,
    route: Signal<Route>,
    username: Signal<Option<String>>,
    role: Signal<Option<String>>,
) -> View {
    let i18n = use_context::<I18n>();
    let form_user = create_signal(String::new());
    let form_pass = create_signal(String::new());
    let error = create_signal(String::new());
    let loading = create_signal(false);

    let i18n_submit = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        if loading.get() {
            return;
        }

        if form_user.get_clone().is_empty() || form_pass.get_clone().is_empty() {
            error.set(i18n_submit.t(K::LoginRequired));
            return;
        }

        loading.set(true);
        let user = form_user.get_clone();
        let pass = form_pass.get_clone();
        let i18n_async = i18n_submit.clone();
        spawn_local_scoped(async move {
            match crate::api::login_api(&user, &pass).await {
                Ok(role_str) => {
                    username.set(Some(user));
                    role.set(Some(role_str));
                    authenticated.set(true);
                    route.set(Route::Dashboard);
                }
                Err(e) => {
                    error.set(i18n_async.t_replace(K::LoginError, "msg", &e.to_string()));
                    loading.set(false);
                }
            }
        });
    };

    auth_page_shell(
        on_submit,
        i18n.t(K::Login),
        (
            auth_input_field(
                i18n.t(K::Username),
                "text",
                i18n.t(K::Username),
                form_user,
                error,
                false,
            ),
            auth_input_field(
                i18n.t(K::Password),
                "password",
                i18n.t(K::Password),
                form_pass,
                error,
                true,
            ),
            auth_error_display(error),
            auth_submit_button(loading, i18n.t(K::LoginBtn)),
            auth_link_footer(
                i18n.t(K::NoAccountRegister),
                i18n.t(K::Register),
                move |_| route.set(Route::Register),
            ),
        ),
    )
}
