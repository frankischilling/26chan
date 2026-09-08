#!/usr/bin/env bash
set -euo pipefail
: "${TEST_PUBLIC_DATABASE_URL:?Set the disposable public test database URL}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --all-features --locked
npm ci --ignore-scripts
npm run test:behavior
