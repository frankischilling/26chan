#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Compare the unchanged parser configuration with the opt-in metadata feature.
# Upstream library tests keep their original assertions and fixtures.
cargo check --manifest-path vendor/hyper/Cargo.toml --locked --no-default-features
for features in client,server,http1 client,server,http1,server-header-order; do
  cargo test --manifest-path vendor/hyper/Cargo.toml --locked --no-default-features \
    --features "$features" --lib
done
