# Cross-board quote navigation

Implemented under [issue #54](https://github.com/frankischilling/26chan/issues/54).
The [official quoting FAQ](https://4chan.org/faq#quote) documents `>>123` for a
post on the current board and `>>>/po/123` for a post on another board. The
[reference manifest](reference-manifest.json) records the September 13 response
hash and collection time. The response matches the previously collected FAQ.
Python's first fetch returned HTTP 403; the ordinary PowerShell web request
succeeded. No external posts or user uploads were collected.

The shared domain parser emits a separate `CrossQuote(board, id)` token. It
accepts the existing 1-to-10 lowercase ASCII letter/digit board identifiers and
positive post numbers within PostgreSQL's signed 64-bit range. Board delimiter
inspection is limited to 11 bytes and number inspection to 20 bytes. The existing
16,000-scalar comment bound still applies. Malformed syntax stays text, HTML is
escaped, and spoiler contents remain nonrecursive text. Existing same-board
quotes and ordinary links retain their behavior.

Public HTML and JSON `com` rendering use the same Askama formatting macro. Cross
quotes link to `/{target_board}/post/{id}`. That existing route checks the target
board and visible post/thread in PostgreSQL before issuing a 303 to the thread
and `#p{id}`. It does not fetch a remote board. A number belonging to another
board, a missing board, or a deleted target returns 404. Quote text in the source
post remains intact after target deletion; the link does not expose deleted text.
Staff previews render the token as escaped text, consistent with same-board
quotes, without a misleading staff-origin content link.

The FAQ establishes syntax and cross-board linking. The local slug/number
bounds, redirect route/status, malformed-input treatment, punctuation suffixes,
nonrecursive spoilers and staff rendering are project decisions. They do not
establish exact original parser, URL or inline-extension parity. No schema,
credential, CSP, external connection or media-processing authority changes.

## Verification

The domain suite passed nine tests, including 128-case properties for cross-board
token round trips and arbitrary prefixed Unicode input, plus existing Unicode
bounds and formatting tests. Staff library tests passed all six cases, including
escaped quote/hostile-text previews. The focused real-browser test passed with
JavaScript disabled:

```text
cargo test -p board-domain --locked --jobs 1
cargo test -p board-staff --lib --locked --jobs 1
cargo clippy -p board-domain -p board-public -p board-staff --all-targets --all-features --locked --jobs 1 -- -D warnings
npx playwright test tests/browser/behavior.spec.js --grep 'cross-board quotes'
```

The browser creates a target thread and reply on one board and a source thread
on another through actual posting forms. It checks persisted HTML and JSON,
reload, the exact quote link and redirect, navigation to the reply, wrong-board
and missing-board denials, spoiler non-linking, escaped hostile text, and target
deletion followed by a 404 navigation. It deletes its own synthetic threads in
cleanup. The production route/store implementation remains unchanged; the test
does not replace it with mocked responses.

The final local regression also passed all 51 public tests, 28 staff tests and
nine real-server browser scenarios against the owned disposable PostgreSQL
cluster. All 29 screenshot comparisons passed without baseline changes:

```powershell
cargo test -p board-public --all-features --locked --jobs 1
cargo test -p board-staff --all-features --locked --jobs 1
npm run test:behavior
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:themes
```

Formatting, JavaScript syntax, whitespace and the media parser dependency guard
passed. No dependency version, screenshot, CI permission or processing boundary
changed. [PR #55](https://github.com/frankischilling/26chan/pull/55) merged as
`db2111153eab0d95701bd27bb0b55767a3732392` after reviewed head `c0326de`
passed complete [PR](https://github.com/frankischilling/26chan/actions/runs/34739166289)
and [push](https://github.com/frankischilling/26chan/actions/runs/34739164526)
Linux/Windows builds and both monitoring workflows. The path-filtered advisory
workflow did not run for this dependency-unchanged slice. The main merge-preview
tree matched the tested head. No production readiness or complete reference
parity claim is made.
