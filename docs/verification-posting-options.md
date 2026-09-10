# Documented posting options

The [official FAQ](https://www.4chan.org/faq#nonoko) documents returning to the
board with `nonoko`, and combining that with sage using `nonokosage`. The
existing local handler accepted only empty or `sage` and returned 422 for the
other two values. [Issue #28](https://github.com/frankischilling/26chan/issues/28)
tracks the correction; [issue #6](https://github.com/frankischilling/26chan/issues/6)
tracks the broader reference work.

The handler now accepts these exact values through both `/{board}/post` and
`/{board}/imgboard.php`, and the Options control exposes each choice:

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
fixtures. The exact allowlist and unsupported-value response remain local
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
