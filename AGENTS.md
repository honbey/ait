# Ait — Agent Guide

## Project Overview

Proxy service for aggregating LLM provider APIs (OpenAI-compatible). Backend in Rust (Axum), frontend WASM CSR in Leptos 0.8.

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

- Use ASCII punctuation (-, ->, ...) not Unicode (—, →, …)
- Prefer self-documenting code; no abbreviations unless widely accepted.
- Check surrounding context (especially imports) before editing; keep changes idiomatic to the framework in use.

### Git

- **Commit msg standards**: see `.gitmessage`
- Squash merge: single comprehensive commit message describing final changes; omit intermediate bugfixes.
- Before committing, run all checks from Commands above (including `leptosfmt frontend/src/`).

### Backend (`src/`)

- **Database**: SQLite (via rusqlite, `Arc<Mutex<Connection>>`) for app data (providers, models, api keys). DuckDB (via `LogManager`) for structured logging + analytics. All DB ops go through `crate::run_blocking()`: SQLite requires acquiring `Mutex<Connection>` on a blocking thread, and DuckDB queries are blocking IO that would stall the tokio runtime.
- **DashMap deadlock prevention**: Never hold a `DashMap` `Ref`/`RefMut` across an `.await` point, and never call `insert`/`retain`/`remove` on the same map while holding a `Ref`/`RefMut` from it — parking_lot's RwLock is **not** reentrant, even on the same thread. This includes sneaky cases like match scrutinee temporaries: `match map.get_mut(k) { Some(mut entry) if stale => ..., _ => map.insert(k, v) }` — the `_` arm still holds the `RefMut` from the scrutinee because match temporaries live until the end of the entire match expression. Always `clone()` the needed data and `drop()` the guard before any insert/retain on the same map.
- **Handler modules**: `providers` (provider CRUD), `models` (model CRUD), `analytics`, `apikeys`, `stats`, `proxy` (proxy internals: `exec.rs`, `sse.rs`, `guard.rs`)
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
