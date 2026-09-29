#!/usr/bin/env bash
set -euo pipefail
: "${TEST_PUBLIC_DATABASE_URL:?Set the disposable public test database URL}"
: "${MEDIA_DATABASE_URL:?Set the disposable media test database URL}"
: "${MEDIA_READ_DATABASE_URL:?Set the disposable approved-media reader database URL}"
: "${INTAKE_DATABASE_URL:?Set the disposable capability-scoped intake database URL}"
: "${MONITOR_DATABASE_URL:?Set the disposable aggregate observer database URL}"
: "${AUTH_DATABASE_URL:?Set the disposable authentication test database URL}"
: "${STAFF_DATABASE_URL:?Set the disposable staff test database URL}"
npm ci --ignore-scripts
if [[ ${CI:-} == true ]]; then
  npx playwright install --with-deps chromium
fi
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
python3 scripts/check-media-parser-dependencies.py
cargo build --workspace --examples --bins --locked
cargo test --workspace --all-features --locked
npm run test:linkification
npm run test:quote-preview
npm run test:backlinks
npm run test:inline-quotes
npm run test:images-core
npm run test:display
npm run check:native-thread-controls
npm run test:thread-updater-dom
npm run test:expansion
npm run test:stats
npm run test:navigation
npm run test:layout
npm run test:depager
npm run test:embeds
npm run test:custom-css
npm run test:settings-transfer
npx playwright test --config playwright.media-visual.config.js tests/media-visual/native-images.spec.js
npm run test:quick-reply
npm run test:behavior
npx playwright test tests/browser/catalog-teasers.spec.js
npx playwright test tests/browser/text-catalog.spec.js
npx playwright test tests/browser/catalog-previews.spec.js
npx playwright test --config playwright.staff.config.js
