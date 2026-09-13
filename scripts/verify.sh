#!/usr/bin/env bash
set -euo pipefail
: "${TEST_PUBLIC_DATABASE_URL:?Set the disposable public test database URL}"
: "${MEDIA_DATABASE_URL:?Set the disposable media test database URL}"
: "${MEDIA_READ_DATABASE_URL:?Set the disposable approved-media reader database URL}"
: "${INTAKE_DATABASE_URL:?Set the disposable capability-scoped intake database URL}"
: "${MONITOR_DATABASE_URL:?Set the disposable aggregate observer database URL}"
: "${AUTH_DATABASE_URL:?Set the disposable authentication test database URL}"
: "${STAFF_DATABASE_URL:?Set the disposable staff test database URL}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --examples --bins --locked
cargo test --workspace --all-features --locked
npm ci --ignore-scripts
npm run test:behavior
npx playwright test --config playwright.staff.config.js
