#!/usr/bin/env bash
set -euo pipefail
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
    mkdir -p "$CARGO_HOME"
    curl --fail --location --retry 3 https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o "$CARGO_HOME/rustup-init"
    chmod +x "$CARGO_HOME/rustup-init"
    "$CARGO_HOME/rustup-init" -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION"
fi
rustup toolchain install "$RUST_VERSION" --profile minimal --component rust-docs
rustup default "$RUST_VERSION"
export CARGO_BUILD_JOBS=$CAPY_BUILD_JOBS
cargo install --locked cargo-about --version 0.9.2 --features cli
node apps/layer-linux/package.mjs
cp -a dist/capycanvas-linux/bin dist/capycanvas-linux/lib dist/capycanvas-linux/share /app/
