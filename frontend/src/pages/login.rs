use leptos::prelude::*;
use leptos_router::components::{A, Redirect};
use leptos_router::hooks::use_navigate;

use crate::api;
use crate::auth::{AuthContext, AuthStatus};
use crate::components::error_display::ErrorText;
use crate::components::style::{CLASS_CARD, CLASS_INPUT, CLASS_LABEL};
use crate::components::use_page_title;
use crate::{t, trs, ts};

#[component]
pub fn LoginPage() -> impl IntoView {
    use_page_title(move || format!("Ait - {}", t!(Login)()));
    let auth = use_context::<AuthContext>().expect("AuthContext");

    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());

    let navigate = use_navigate();
    let auth_redirect = auth.clone();

    let login_action: Action<(String, String), Result<(), String>> =
        Action::new_local(|input: &(String, String)| {
            let (u, p) = input.clone();
            async move { api::login_api(&u, &p).await.map_err(|e| e.to_string()) }
        });

    let consumed = RwSignal::new(false);

    Effect::new(move |_| {
        if login_action.pending().get() {
            consumed.set(false);
            return;
        }
        if consumed.get_untracked() {
            return;
        }
        match login_action.value().get() {
            Some(Ok(())) => {
                consumed.set(true);
                let u = username.get_untracked();
                auth.set_logged_in(u);
                navigate("/console", Default::default());
            }
            Some(Err(e)) => {
                consumed.set(true);
                error.set(trs!(LoginError, &[("msg", &e)]));
            }
            None => {}
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        if login_action.pending().get_untracked() {
            return;
        }

        let u = username.get_untracked();
        let p = password.get_untracked();
        if u.is_empty() || p.is_empty() {
            error.set(ts!(LoginRequired));
            return;
        }

        error.set(String::new());
        login_action.dispatch((u, p));
    };

    view! {
        <Show when=move || auth_redirect.authenticated.get() == AuthStatus::Authenticated>
            <Redirect path="/console" />
        </Show>
        <main>
            <div class="min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900">
                <form on:submit=on_submit class=format!("{} p-8 w-full max-w-md mx-4", CLASS_CARD)>
                    <h2 class="text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6 text-center">
                        {t!(Login)}
                    </h2>

                    <div class="mb-4">
                        <label for="login-username" class=CLASS_LABEL>
                            {t!(Username)}
                        </label>
                        <input
                            id="login-username"
                            name="username"
                            type="text"
                            autocomplete="username"
                            placeholder=t!(Username)
                            prop:value=move || username.get()
                            on:input=move |ev| {
                                username.set(event_target_value(&ev));
                                error.set(String::new());
                            }
                            class=CLASS_INPUT
                        />
                    </div>

                    <div class="mb-6">
                        <label for="login-password" class=CLASS_LABEL>
                            {t!(Password)}
                        </label>
                        <input
                            id="login-password"
                            name="password"
                            type="password"
                            autocomplete="current-password"
                            placeholder=t!(Password)
                            prop:value=move || password.get()
                            on:input=move |ev| {
                                password.set(event_target_value(&ev));
                                error.set(String::new());
                            }
                            class=CLASS_INPUT
                        />
                    </div>

                    <ErrorText msg=error />

                    <button
                        type="submit"
                        disabled=move || login_action.pending().get()
                        class="w-full py-2 px-4 bg-indigo-600 hover:enabled:bg-indigo-700 text-white font-semibold rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 cursor-pointer active:scale-95"
                    >
                        <Show when=move || login_action.pending().get()>
                            <i class="fas fa-spinner animate-spin"></i>
                        </Show>
                        <span>{t!(Login)}</span>
                    </button>

                    <div class="mt-4 text-center">
                        <A
                            href="/"
                            {..}
                            class="text-sm text-indigo-600 dark:text-indigo-400 hover:underline"
                        >
                            {t!(Index)}
                        </A>
                    </div>
                </form>
            </div>
        </main>
    }
}
