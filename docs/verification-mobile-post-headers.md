# Mobile post-header verification

The September 30 change renders paired mobile and desktop headers from the same
stored post. It covers short labels, staff badges, tripcodes, poster IDs, flags,
dates, number links, menu movement, dark mode and inert quote copies. Watcher
labels and name filters read the full desktop fields behind the presentation.

## Reference and environment

The [reference notes](source-mobile-headers.md) identify the public v1191 client,
six desktop styles, two mobile styles and nine fixed icons. The replay scripts
check their hashes before running the inspected formatter or pure helpers.
Windows and Linux replayed all 288 mobile property cases and ten label vectors
against the recorded JSON. Neither run initializes the released extension or
requests external content.

Local checks use Rust 1.94.0, Node 24.14.0, Playwright 1.62.0 and Chromium
151.0.7922.34. Property cases use 390 × 844, density 1; icon-fit cases also use
density 2. The Windows screenshots use the existing pinned viewport, locale,
fonts and zero-pixel comparison settings. Browser retries remain disabled.

## Completed checks

| Command | Actual result |
| --- | --- |
| `node scripts/verify-public-mobile-label-reference.mjs <pinned-client>` | Exact Windows and Linux replay: ten vectors and one serialized comma control |
| `node scripts/verify-public-mobile-header-reference.mjs <pinned-client> <desktop-css-dir> <mobile-css-dir>` | Exact Windows and Linux replay: 288 cases |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` | Passed |
| `node --test --test-concurrency=2 tests/browser/*.test.mjs apps/public/client/native-thread-hiding-state.test.js apps/public/client/native-keybinds.test.js` | All 394 Linux tests passed; none skipped |
| Native filter, Quick Reply and thread-control bundle checks | Passed; generated assets match the source and retain their size limits |
| `node node_modules/@playwright/test/cli.js test tests/browser/mobile-post-headers.spec.js tests/browser/native-quote-preview.spec.js tests/browser/native-backlinks.spec.js` | All 86 Linux persisted-page tests passed: four mobile cases and 82 preview/backlink cases |
| `node node_modules/@playwright/test/cli.js test --config playwright.themes.config.js` | All 182 Windows tests passed, including 288 mobile and 72 desktop header comparisons |
| `node node_modules/@playwright/test/cli.js test --config playwright.media-visual.config.js` | All 57 Windows tests passed |
| `node node_modules/@playwright/test/cli.js test --config playwright.archive-visual.config.js` | All six Windows tests passed |
| `node node_modules/@playwright/test/cli.js test --config playwright.states.config.js` | All ten Windows tests passed |
| `node node_modules/@playwright/test/cli.js test tests/browser/visual.spec.js` with the visual fixture server | All three Windows tests passed |
| Linux production header properties | All 32 tests passed |
| Linux persisted display, IDs, posting preferences, menus and mobile headers | All 42 tests passed after correcting the watcher tuple assertion |
| `node node_modules/@playwright/test/cli.js test --config playwright.inline-quotes.config.js tests/browser/native-inline-quotes.spec.js` | All 36 Linux tests passed |
| `node node_modules/@playwright/test/cli.js test --config playwright.staff.config.js tests/browser/staff.spec.js` | The Linux WebAuthn, moderation, recovery and logout test passed |
| Corrected filter asset checks and complete JavaScript suite | All three assets matched their pinned sources; all 394 tests passed again |
| `node node_modules/@playwright/test/cli.js test tests/browser/native-reply-hiding.spec.js tests/browser/native-page-filters.spec.js` | All 25 Linux tests passed, including mobile View placement and desktop/mobile board stubs |
| Corrected word-break, catalog-teaser, text-catalog and catalog-preview cases | All five Linux tests passed; JavaScript-disabled and Quick Reply word-break paths remain covered |

The [first hosted run](https://github.com/frankischilling/26chan/actions/runs/36740994438)
passed the full Rust workspace phase: 544 tests across 154 result groups, with
none ignored. Windows visual and privileged media qualification also passed.
Its later browser phase failed because the filter `[View]` control stayed in
the hidden desktop header at mobile width. The same failure reproduced locally.
The control now follows the displayed header; the regression checks preserve
its identity across 480/481px and actual cross-tab opt-out changes, then require
manual hiding to remain independent. Board-thread filter stubs are checked at
desktop and mobile widths. The 394 JavaScript and 25 filter/hiding tests passed
after this correction. Fresh exact-head hosted qualification is pending at this
checkpoint; final results are recorded on the pull request before merge.

The [second hosted run](https://github.com/frankischilling/26chan/actions/runs/36746795945)
passed the full 544-test Rust phase and the corrected filter/hiding cases. Its
later word-break browser case switched to 390px, then tried to click the hidden
desktop reply link. The trace records that click without a completion event;
the unchanged case reproduced locally. The selector now chooses the visible
original header. Posting, stored formatting, API, catalog and Quick Reply
assertions remain intact, as do the 30-second case limit and zero retries.

An earlier local complete Rust run failed when a newly generated TLS
certificate was reported as not yet valid. The unchanged focused proxy test
passed afterward. The original cause remains unconfirmed; no clock, TLS
verification, retry or deadline policy changed.

## Screenshot review

Twelve mobile images changed: the six theme preference pages, the basic board,
empty board, archived text thread, and three attachment board/thread/archive
pages. Each actual image and difference was inspected before copying that
specific baseline. The differences follow the separate mobile header, Arial
body font, worksafe mobile colors, full-width posts and hidden reply arrows.
The OP header now precedes its file. Existing catalog and desktop baselines
passed after restricting mobile post styles to board and thread pages.

The first full Windows run exposed that catalog scope error. It also found
desktop color expectations in the mobile preference test; those now assert the
pinned mobile colors while retaining the selected desktop stylesheet checks.
A desktop archive screenshot timed out once during capture. Its baseline stayed
unchanged, and the later six-case archive run passed without a retry or timeout
change. This does not establish the cause of that first capture failure.

## Negative and concurrency checks

The finite recipe rejects foreign header IDs, mixed quote targets, forged short
labels or titles, mismatched timestamps and identity fields, incomplete staff
badges and post-number markers in the body. Copies lose IDs, forms and original
controls. ID actions require matching container, post and header IDs and count
each original post once across both headers. Parser bytes, nodes, depth, request
deadlines and the Quick Reply asset ceiling retain their existing bounds.

The workspace run also encountered an attachment-expiry assertion failure.
Its focused diagnostic run passed, so the original cause remains unconfirmed.
The test now waits, within five seconds, for both an actual PostgreSQL blocker
and expiry on the database wall clock before releasing the job lock. The
waiting public write must still return `NotFound`. The focused test passed with
those witnesses. No attachment authorization function or runtime limit changed.

These are local and synthetic checks. The mobile property reference does not
qualify original full-page pixels, original-server name serialization, the
released custom tooltip callback or production deployment. Issues #5 and #6
remain open. Exact-head hosted checks are required before merge.
