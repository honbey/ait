# Ait — Agent Guide

## Project Overview

Proxy service for aggregating LLM provider APIs (OpenAI-compatible). Backend in Rust (Axum), frontend WASM CSR in Sycamore 0.9.

## Commands

```bash
cargo check                     # Backend
cargo check --target wasm32-unknown-unknown -p ait-frontend  # Frontend
cargo clippy                    # Backend lint (no warnings)
cargo clippy --target wasm32-unknown-unknown -p ait-frontend  # Frontend lint
cargo test                      # All tests
cargo fmt                       # Format both
trunk build                     # Frontend dev (from frontend/), you use it
trunk watch                     # Frontend dev (from frontend/), user use it
trunk build --release --cargo-profile release-wasm  # Frontend prod
```

## Guidelines

### Code Style

- Keep code comments concise:
  - Avoid repetitive or excessive inline explanations
  - Except `//` for complex logic, exceptional cases or doc comments
- Use ASCII characters(`-`, `->`, `...`) not Unicode(`—`, `→`, `…`)
- Keep code consistency (coding style, functional logic, component structure, error handling, etc.).

### Git

- When performing a squash merge (or using a PR to squash merge):
  - Use a single comprehensive commit message that directly describes the final changes made
  - There is no need to mention bugs that were introduced and subsequently fixed during the development process
- Before committing, run `cargo check`, `cargo clippy`, and `cargo fmt` to ensure code quality and formatting
- After the pre-commit checks pass, wait for user review. The commit may be submitted only after the user has confirmed approval
- Commit msg standards: see `.gitmessage`

### Backend (`src/`)

- RocksDB is `Send + Sync` (via `Arc`) for CRUD storage
  - Single `get_cf` (~10–50µs) can run on tokio worker, writes and batch reads use `spawn_blocking`
- DuckDB (via `LogManager`) for structured logging and analytics
- All handlers receive `Extension<Arc<Config>>`, `Extension<Database>`, `Extension<LogManager>`
- API routes `/*` (via `/api/` prefix), proxy routes `/v1/*`
- New providers: implement trait + register in `providers/mod.rs`

### Frontend (`frontend/src/`)

- Sycamore reactive signals, `View::from_dynamic` minimal (avoid closure-in-closure)
- Use `Rc<Vec>` for shared data passed to modals/children
- Use `create_client_resource` for async fetching in layout views
- Loading states: Level-1 SVG spinner (session check) -> Level-2 skeleton (page data) -> actual view
- `Index`, `Login`, `Register` are public routes
- Routes under `/console/` are protected (auth guard in `layout.rs`)
- ECharts loads lazily (dynamic `<script>` injection in `LineChart` mount)

#### i18n

- Add new key to `frontend/locales/zh.json` and other `lang.json`
- Keys are compile-time checked (`build.rs` generates `K` enum)
- Access via `i18n.t(K::Variant)` or `i18n.t_replace(K::Variant, "placeholder", &value)`

### Other

Read `.agents.local.md`.
