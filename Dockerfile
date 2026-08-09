# syntax=docker/dockerfile:1
# Multi-stage build: frontend (WASM via Trunk), backend (native), runtime.

# ---------- Stage 1: frontend WASM build ----------
FROM rust:1-slim AS frontend-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl unzip \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown

ARG TRUNK_VERSION=v0.21.14
RUN curl -sSfLo /tmp/trunk.tar.gz \
        https://github.com/trunk-rs/trunk/releases/download/${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz \
    && tar xzf /tmp/trunk.tar.gz -C /usr/local/bin \
    && rm /tmp/trunk.tar.gz

WORKDIR /app
COPY . .

RUN --mount=type=cache,id=ait-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ait-frontend-target,target=/app/target \
    cd frontend && trunk build --release --cargo-profile release-wasm

# ---------- Stage 2: backend native build ----------
FROM rust:1-slim AS backend-builder

# rusqlite and duckdb are compiled bundled, which needs a C/C++ toolchain
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN --mount=type=cache,id=ait-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ait-backend-target,target=/app/target \
    cargo build --release \
    && cp /app/target/release/ait /app/ait

# ---------- Stage 3: runtime ----------
FROM debian:trixie-slim AS runtime

USER root
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist
COPY --from=backend-builder /app/ait /app/ait
COPY config/ait.toml.example /app/config/ait.toml

RUN mkdir -p /app/data

# Listen on all interfaces inside the container (default binds to 127.0.0.1)
ENV AIT_SERVER_HOST=0.0.0.0

EXPOSE 8000

# The binary only handles SIGINT for graceful shutdown
STOPSIGNAL SIGINT

# SQLite database + DuckDB logs
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${AIT_SERVER_PORT:-8000}/health" || exit 1

CMD ["/app/ait"]
