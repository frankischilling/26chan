# Posting options

## Supplied-source matching

[Issue #112](https://github.com/frankischilling/26chan/issues/112) covers the
raw Options field and public parsing established by the supplied source:

- `imgboard.php:5299,5304` bounds ordinary public options at 100 bytes.
- `imgboard.php:5413-5424` removes every case-insensitive ASCII `sage`
  substring, setting the sage flag if any occurred. The remainder must equal
  `nonoko` case-insensitively for a board return. It does not trim whitespace.
- `imgboard.php:5605-5640` handles the case-sensitive `capcode_` prefix and
  clears the options before storage. Unauthenticated public attempts cannot
  receive capcodes (`parse_capcode`, lines 4750-4809); the name becomes
  Anonymous. The rewrite grants no staff authority through public options.
- `views/imgboard.php:84-119` uses a text input, with the reply submit control
  beside Options and the new-thread submit control beside Subject.

Both posting aliases apply these parsing rules. `sageNONOKOSaGe` suppresses
bumping and returns to the board. `nonoko sage` suppresses bumping but returns
to the thread because the space remains. `message` also contains `sage`.
Other bounded text is accepted without storing or displaying raw options.
Inputs over 100 UTF-8 bytes receive 422. Only fixed same-origin success
destinations are available; HTML 303 and JSON response handling are unchanged.

The parser has example tests and 128 bounded property cases. The database
test covers eleven values through both aliases for OPs and replies, actual
bump timestamps and redirects, 100/101-byte boundaries, closed-thread
rejection, and unprivileged capcode attempts with an uppercase control.
Browser coverage includes free-text fields, reply submit placement, and
persisted no-JavaScript board returns with mixed-case options.

Local checks passed all 16 domain tests, 35 public library tests, all-target
Clippy, 39 media/interaction tests, ten state tests and three base visuals.
Six form-layout cases passed, including the added input and submit assertions.
Eighteen reviewed screenshot baselines cover the affected expanded forms:
four attachment board/thread, two empty-board and twelve six-theme captures.
The initial full theme run passed 116 cases and failed only those two
six-theme screenshot groups. After their scoped update, the complete rerun
passed all 118 cases in 3.0 minutes without updating snapshots.
Local PostgreSQL is unavailable, so the expanded persisted tests require
current-head CI.
Fixture screenshots are project regressions, not original rendered-page
parity evidence. Further [bump policies](source-bump-rules.md), identity/capcode cookies, authenticated
capcodes, special board options, Pass/captcha and complete forms remain
unfinished. This change adds no schema, grants, dependencies or upload authority.

## Earlier FAQ-only qualification

The following records the narrower implementation and checks performed for
issue #28. Its exact allowlist and selector are superseded by the source
rules above; these historical test results do not qualify the expanded parser.

The [official FAQ](https://www.4chan.org/faq#nonoko) documents returning to the
board with `nonoko`, and combining that with sage using `nonokosage`. The
existing local handler accepted only empty or `sage` and returned 422 for the
other two values. [Issue #28](https://github.com/frankischilling/26chan/issues/28)
tracks the correction; [issue #6](https://github.com/frankischilling/26chan/issues/6)
tracks the broader reference work.

That implementation accepted these exact values through both `/{board}/post`
and `/{board}/imgboard.php`, with a selector exposing each choice:

| Value | Sage flag | Local success destination |
| --- | --- | --- |
| Empty | False | The submitted post in its thread |
| `sage` | True | The submitted post in its thread |
| `nonoko` | False | Board index |
| `nonokosage` | True | Board index |

Redirects occur only after persisted creation succeeds. Existing 303 status,
relative same-origin destinations, validation, database limits and deletion
credentials remain. Unsupported values receive 422 with an updated list of
choices. No arbitrary redirect target is accepted. Sage affects replies under
the existing store rules; it does not prevent creating a new thread.

## Reference provenance

The FAQ response was retrieved as public HTML on September 10, 2026 at
01:38:38.3040609 UTC, redirecting from `www.4chan.org/faq` to `4chan.org/faq`.
Its 78,050 bytes have SHA-256
`85256d4e97488f565e9ec63472c4e8f06a0afa0d332d3ea71cd18f29f6dbced8`.
[The manifest](reference-manifest.json) records this separately from the pinned
read-only API revision. The original response is retained locally outside
version control; its complete text is not redistributed. The three recorded
fragment identifiers were checked against that response.

This source supplies documented option meanings, not observed HTTP statuses,
case folding, whitespace rules, arbitrary-email behavior or rendered form
layout. No test posted to the external site or collected production post/media
fixtures. That exact allowlist and unsupported-value response were local
validation policy. Full visual and behavioral parity is still unverified.

## Tests and failures

The new actual-database test first failed on both added options through both
posting aliases. Existing empty/sage controls succeeded. Its first cleanup
also failed because deletion-password rows still referenced the generated
posts. Cleanup now removes those owned secret rows before posts, threads and
the board. The retained fixture was identified by its recorded post ID and
generated board, then its expected metadata, row counts and synthetic comments
were checked before transactional removal. The repeated red run failed only
for the unsupported options and left no fixture board behind.

After implementation, the focused test passed in 7.76 seconds. It exercises
sixteen successful creations: four options, both aliases, and new threads plus
replies. A fixed old bump timestamp avoids timing-dependent comparisons.
Assertions check stored post/thread membership, anonymous names, counts,
redirects, sage suppression and successful non-sage bumps. Six invalid-value
requests and two closed-thread requests verify denial without persistence or
success redirects.

The Windows browser case uses JavaScript-disabled forms for both added options,
checks the actual POST response and board destination, reloads both views to
confirm persistence, and deletes its owned thread in cleanup. All six behavior
tests and all three existing visual tests passed; no screenshot baseline was
changed. These remain synthetic project screenshots, not original-site parity
evidence. Final inventory found no generated database fixture board or public
test server listening on ports 3000/3003.

## Reproduction and verification

Native checks used the existing disposable PostgreSQL 16.15 database and Rust
1.94.0. Source the existing ignored test environments privately; no credentials
were rotated or logged:

```sh
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/staff.env
export CARGO_HOME=/opt/26chan-rust/cargo
export RUSTUP_HOME=/opt/26chan-rust/rustup
export CARGO_TARGET_DIR=/opt/26chan-rust/target
export PATH=/opt/26chan-rust/cargo/bin:$PATH
cargo test -p board-public --features database-tests --test posting_options --locked
cargo test -p board-public -p board-store --all-features --all-targets --locked
cargo clippy -p board-public -p board-store --all-features --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The complete run passed 29 public tests and eight store tests, including the
new test in 10.10 seconds. Scoped Clippy passed. Windows `cargo build -p
board-public --locked` passed, followed by `npm run test:behavior` (six passed,
13.6 seconds) and `npm run test:visual` (three passed, 4.9 seconds). The private
Windows environment importer cleared `VISUAL_FIXTURE_SERVER`; both suites used
the actual public server and disposable database. Playwright managed server
startup and shutdown. Existing CI discovers the new tests without workflow
changes; hosted results are tracked in the draft PR linked from issue #28.

A separate source review found no runtime, security or cleanup blockers and
confirmed the retained FAQ provenance. Two documentation corrections preserved
existing requirement IDs and included the selected FAQ sections in the
documented-evidence definition. The reviewer did not execute tests; the
execution results above came from the implementation checks.

No schema, grants, registry dependency, stylesheet or production setting
changed. Public media remains disabled. This implements the documented options
without claiming completion of the remaining compatibility or launch gates.
