#!/usr/bin/env bash
set -euo pipefail
# This runner only qualifies owned disposable development databases.
if [[ ${APP_ENV:-development} != development ]]; then
  echo 'Verification requires development mode.' >&2
  exit 1
fi
export APP_ENV=development
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
npm run check:generated
timeout 300s npm run test:math
python3 scripts/check-windows-test-exits.py
npm run test:settings-categories
node --test tests/browser/staff-auth-budget.test.mjs tests/browser/owned-upload-response.test.mjs tests/browser/deletion-quota-fixture.test.mjs tests/helpers/verify-runner.test.mjs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
python3 scripts/check-media-parser-dependencies.py
cargo build --workspace --examples --bins --locked
cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked
cargo test --workspace --all-features --locked
cargo test -p board-staff --features database-tests --test uploads --locked script_disabled_browser -- --ignored --exact --nocapture
cargo test -p board-public --example deletion-fixture --features database-tests --locked
cargo test -p board-public --example deletion-quota-fixture --features browser-tests --locked
# Drawing uses the public browser setup, which requires the built quota fixture.
timeout 300s npm run test:drawing
npx playwright test tests/browser/anonymous-session.spec.js
node --test tests/browser/polls-fixture.test.mjs
npm run test:polls
npm run test:blotter
npm run test:blotter-persisted
npm run test:global-search
npm run test:posting-randomizers
npm run test:robot9000
npm run test:wordfilters
npm run test:linkification
npx playwright test tests/browser/static-quotes.spec.js
npm run test:quote-preview
npm run test:backlinks
npm run test:inline-quotes
npm run test:images-core
npm run test:files-core
npm run test:custom-spoilers
npm run test:source-flags
npm run test:display
npm run test:post-tooltips
npm run check:native-thread-controls
npm run test:source-parsing
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
npx playwright test --config playwright.media-visual.config.js tests/media-visual/file-presentation.spec.js
npm run test:quick-reply
npx playwright test tests/browser/mobile-post-headers.spec.js
npx playwright test tests/browser/post-identities.spec.js
npx playwright test tests/browser/poster-ids.spec.js
npm run test:catalog-filters-core
npm run test:catalog-theme-core
npm run test:behavior
npx playwright test tests/browser/catalog-teasers.spec.js
npx playwright test tests/browser/text-catalog.spec.js
npx playwright test tests/browser/catalog-previews.spec.js
# Harmless inherited values must not reach the separately privileged services.
AZURE_CONFIG_DIR=/tmp/owned-synthetic-cloud-config \
SSH_AUTH_SOCK=/tmp/owned-synthetic-agent \
EXTRA_DATABASE_URL=owned-synthetic-shadow-url \
  npx playwright test --config playwright.staff.config.js
