#!/usr/bin/env bash
set -euo pipefail

# Install trunk (https://github.com/trunk-rs/trunk) from a release tarball.

TRUNK_VERSION="${TRUNK_VERSION:-v0.21.14}"
TRUNK_TARGET="${TRUNK_TARGET:-x86_64-unknown-linux-musl}"

# Install destination. Defaults to a dir on PATH; override with CARGO_BIN.
CARGO_BIN="${CARGO_BIN:-/opt/data/rust/cargo/bin}"

URL="https://github.com/trunk-rs/trunk/releases/download/${TRUNK_VERSION}/trunk-${TRUNK_TARGET}.tar.gz"

rustup target add wasm32-unknown-unknown

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

echo "[trunk] downloading ${URL}"
curl -fsSL "${URL}" -o "${tmp}/trunk.tar.gz"
tar xzf "${tmp}/trunk.tar.gz" -C "${tmp}"

mkdir -p "${CARGO_BIN}"
install -m 0755 "${tmp}/trunk" "${CARGO_BIN}/trunk"

echo "[trunk] installed: $(${CARGO_BIN}/trunk --version)"

# Install leptosfmt (https://github.com/bram209/leptosfmt) from a release tarball.
LEPTOSFMT_VERSION="${LEPTOSFMT_VERSION:-0.1.33}"
LEPTOSFMT_TARGET="${LEPTOSFMT_TARGET:-x86_64-unknown-linux-musl}"

LEPTOSFMT_URL="https://github.com/bram209/leptosfmt/releases/download/${LEPTOSFMT_VERSION}/leptosfmt-${LEPTOSFMT_VERSION}-${LEPTOSFMT_TARGET}.tar.gz"

echo "[leptosfmt] downloading ${LEPTOSFMT_URL}"
curl -fsSL "${LEPTOSFMT_URL}" -o "${tmp}/leptosfmt.tar.gz"
tar xzf "${tmp}/leptosfmt.tar.gz" -C "${tmp}"

install -m 0755 "${tmp}/leptosfmt" "${CARGO_BIN}/leptosfmt"

echo "[leptosfmt] installed: $(${CARGO_BIN}/leptosfmt --version)"

# Install twiggy (https://github.com/rustwasm/twiggy)
#curl -fsSL https://to_be_updated/public/twiggy -o "/tmp/twiggy"
#install -m 0755 "/tmp/twiggy" "${CARGO_BIN}/twiggy"

#echo "[twiggy] installed: $(${CARGO_BIN}/twiggy --version)"