# Ait — Agent Guide

## Project Overview

Proxy service for aggregating LLM provider APIs (OpenAI-compatible). Backend in Rust (Axum), frontend WASM CSR in Leptos 0.8.

**Language**: Reply in the user's language (e.g., zh-CN, en).

## Commands

```bash
cargo check                     # Backend
cargo check --target wasm32-unknown-unknown -p ait-frontend  # Frontend
cargo clippy                    # Backend lint (no warnings)
cargo clippy --target wasm32-unknown-unknown -p ait-frontend  # Frontend lint
cargo test                      # All tests
cargo fmt                       # Format backend only
leptosfmt frontend/src/         # Format frontend Leptos code
trunk build                     # Frontend dev (from frontend/), you use it
trunk watch                     # Frontend dev (from frontend/), user use it
trunk build --release --cargo-profile release-wasm  # Frontend prod
```

> `leptosfmt` is a separate tool — `cargo fmt` does NOT format frontend Leptos view! macros.

## Guidelines

### Code Style

- Keep code comments concise:
  - Avoid repetitive or excessive inline explanations
  - Except `//` for complex logic, exceptional cases or doc comments
- Use ASCII punctuation (`-`, `->`, `...`) not Unicode (`--`, `'->'`, `'...'`)
- For self-documenting code, aim for the code to be as readable as documentation. Do not use abbreviated forms unless they are widely accepted conventions.
- When editing a piece of code, first look at the code's surrounding context (especially its imports) to understand the choice of frameworks and libraries. Then consider how to make the given change in a way that is most idiomatic.

### Git

- **Commit msg standards**: see `.gitmessage`

- When performing a squash merge (or using a PR to squash merge):
  - Use a single comprehensive commit message that directly describes the final changes made
  - There is no need to mention bugs that were introduced and subsequently fixed during the development process
- Before committing, run `cargo check`, `cargo clippy`, `cargo fmt`, and `leptosfmt frontend/src/` to ensure code quality and formatting
- After the pre-commit checks pass, wait for user review. The commit may be submitted only after the user has confirmed approval

### Backend (`src/`)

- **Database**: SQLite (via rusqlite, `Arc<Mutex<Connection>>`) for app data (providers, models, users, api keys, sessions). DuckDB (via `LogManager`) for structured logging + analytics. All DB ops go through `crate::run_blocking()` because SQLite Mutex must be acquired on a blocking thread and bcrypt is CPU-bound.
- **DashMap deadlock prevention**: Never hold a `DashMap` `Ref`/`RefMut` across an `.await` point, and never call `insert`/`retain`/`remove` on the same map while holding a `Ref`/`RefMut` from it — parking_lot's RwLock is **not** reentrant, even on the same thread. This includes sneaky cases like match scrutinee temporaries: `match map.get_mut(k) { Some(mut entry) if stale => ..., _ => map.insert(k, v) }` — the `_` arm still holds the `RefMut` from the scrutinee because match temporaries live until the end of the entire match expression. Always `clone()` the needed data and `drop()` the guard before any insert/retain on the same map.
- **Handler modules**: `providers` (provider CRUD), `models` (model CRUD), `analytics`, `apikeys`, `users`, `auth`, `proxy` (proxy internals: `exec.rs`, `sse.rs`, `guard.rs`)
- API routes under `/api/` prefix, proxy routes under `/v1/*`
- New providers: implement `UpstreamProvider` trait + register in `providers/mod.rs`
- Config: TOML file + env var override (`AIT_<SECTION>_<KEY>`)

### Frontend (`frontend/src/`)

- Leptos 0.8 CSR, `gloo-net` for HTTP, `gloo-storage` for LocalStorage/SessionStorage fallback chain
- Tailwind CSS 4.3 via Trunk pre-build hook, ECharts 6.1 injected dynamically via `<script>`
- Build artifact at `frontend/dist/`, served by backend on `/*`

#### Leptos Reactive Tracking

- In **non-reactive** contexts (component body, `spawn_local`, async blocks, event handler callbacks, `#[prop]` initializers), use `.get_untracked()` / `.with_untracked()` on signals to avoid the "accessed outside a reactive tracking context" warning
- For `LocalResource`, same rule: `.get_untracked()` in non-reactive positions, `.get()` only when used inside a `Transition` child closure or another reactive scope
- Only use `.get()` / `.with()` when inside a `move ||` closure in a `view!` macro that genuinely needs reactive updates (e.g., `t!` macro's `move || i18n().t(K::Foo)`)

#### i18n

- Add new key to `frontend/locales/zh.json` and other `lang.json`
- Keys are compile-time checked (`build.rs` generates `K` enum)
