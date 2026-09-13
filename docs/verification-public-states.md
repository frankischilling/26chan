# Public empty and error states

[Issue #56](https://github.com/frankischilling/26chan/issues/56) covers the empty
board, catalog and directory, an unknown route, and unavailable storage at
1280 by 900 and 390 by 844 pixels. These ten screenshots use synthetic data and
the pinned Windows/Chromium environment in [the reference manifest](reference-manifest.json).
They are project regressions under V-002, not original-site visual references.

An empty catalog now links to `/{board}/#postForm`. The catalog has no posting
form, so its previous instruction to start a thread "above" was unusable. The
empty message spans the catalog grid instead of occupying a single thumbnail
column. The index retains its existing form and message. No populated-page
markup or stylesheet rule changes.

The database test creates its own empty board through the migration identity,
reads both routes through the public application and restricted public login,
then removes that board before evaluating the response assertions. The initial
fixture omitted required board settings and failed its insert; after supplying
them, the test reproduced the misleading catalog message. The template change
made the same test pass. It does not replace the route or database with a mock.

The screenshot server compiles production templates for empty content. Its
fallback uses the actual public router, middleware and error conversion with a
deliberately closed lazy pool. No database connection, credential, service outage
or production change is needed. `/offline/` exercises storage failure; an
unmatched path exercises the normal 404 handler. This verifies error rendering,
not deployed database availability or recovery.

JavaScript-disabled browser scenarios check the HTTP status, visible messages,
absence of fabricated posts/forms, local navigation, editable posting controls,
and no horizontal overflow. Error responses retain the actual script-free CSP
and `nosniff`; rendered pages do not expose the fixture database URL. The tests
follow the catalog link to the real form template and return from error pages
to the directory. Persistence and actual posting remain covered by the separate
database and normal-server browser suites.

## Local verification

```powershell
cargo test -p board-public --all-features --locked --jobs 1 --test empty_states
cargo build -p board-public --example visual-fixtures --locked --jobs 1
npm run test:states
```

The focused database test passed. The first screenshot run reported absent
baselines and exposed two fixture mistakes: loop-level hooks applied the mobile
viewport to desktop cases, and the 11-character `unavailable` slug returned 404
before reaching storage. Each scenario now sets its own viewport and uses the
valid `offline` slug. Expected statuses and pixel tolerances were not relaxed.

All ten final captures were individually inspected. That review also exposed
the narrow empty catalog column, corrected with an empty-state-only grid rule.
After inspecting the corrected capture, only this new suite was regenerated
with `npm run test:states -- --update-snapshots`; all ten scenarios passed,
including recovery navigation after capture. No prior baseline was refreshed.

The full public regression passed all 52 tests, clippy with warnings denied,
nine normal-server browser scenarios, and 39 screenshot comparisons in 29
scenarios. The ten new comparisons passed without updates, followed by all 29
unchanged board, archive, attachment and theme comparisons:

```powershell
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
npm run test:behavior
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:states
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:themes
cargo fmt --all -- --check
node --check tests/public-states/states.spec.js
```

Formatting, JavaScript syntax and whitespace checks passed. Exact-head hosted
checks remain required. The Windows CI job now includes `npm run test:states`.
Linux runs the actual database test
through the existing all-feature workspace suite; Windows screenshots do not
establish Linux or original-site visual parity. No dependency, migration,
credential, production media setting or deployed service changes.
