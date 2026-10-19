# Janitor review shortcuts

The request queue provides the source Warn, Ban, Global and Short actions. Each
is an ordinary POST form, so the controls also work without JavaScript. With
JavaScript enabled they use the existing three-second review delay and Undo.
The W key invokes the focused row's available Warn action.

Warn amends the requested template to zero days and leaves scope unspecified.
Ban converts a warning request to a one-day global ban. Global retains the
selected template's positive whole-day duration and explicitly selects global
scope. Short changes source template 135 to template 207, with one day and global
scope. The ordinary Accept action is unchanged.

The source's `global1` rule suppresses Warn. Migration 0212 adds that boolean to
the existing role-scoped queue metadata; it does not expose private reasons,
addresses or retained media. Disabled, unavailable or higher-rank templates
cannot supply shortcuts. Short requires an available template 207. The writer
rechecks configuration and authority when the form is submitted.

The native review endpoint accepts whole days from -1 through 1024. Global is
therefore omitted for fractional-day, indefinite, zero-day and out-of-range
templates. The amendment form remains available. No shortcut rounds a duration
or silently substitutes a different template.

## Source checks

`fixtures/janitor-shortcut-source.json` pins the supplied ban-request JavaScript
file and its isolated warning decisions, menu construction and click handler.
The source fixture runs without the original application. Its 160 combinations
cover requested warnings, template warnings, scope, the global1 flag, ordinary
and evasion templates, and durations of -1, 0, 1, 7 and 1024 days.

```sh
node scripts/test-janitor-shortcut-source.mjs /path/to/4chan-old
node --test tests/browser/janitor-queue-state.test.mjs
cargo test -p board-staff --lib --locked
REQUEST_FILE_EFFECTS=1 bash scripts/test-janitor-request-review.sh
cargo test -p board-staff --features database-tests --test janitor_requests --locked
```

The Rust tests compare the native shortcut payloads with all source cases.
Additional controls cover unavailable configuration and fractional-day refusal.
The database suite checks migration rollback, the global1 metadata, review
transactions, contention and populated restore. The HTTP fixture checks rendered
controls and submits a global amendment through the authenticated endpoint.
The browser fixture exercises warning cancellation alongside ordinary reviews.

Issue #223 remains open. Private retained media, template autocomplete, custom
menus, key remapping, thumbnail controls, staff-history tools and complete source
visual comparison still need implementation and qualification.

## Qualification

Local checks passed: 61 staff unit tests, 11 queue-state tests, the 160-case
source oracle, strict all-target staff Clippy, and the PostgreSQL/HTTP request
fixture. The migration suite also passed its rollback, metadata, contention and
populated restore checks. Local Chromium is blocked at Unix socket creation;
desktop/mobile browser qualification must pass in hosted CI before merge.
