# Comment character-limit verification

This checkpoint addresses compatibility I-001 and B-003 and narrows exception E-005. The advertised `max_comment_chars` setting now governs posting as a Unicode scalar count. A board allowing 4,000 characters accepts 4,000 instances of `é` or `😀`; a combining mark, modifier or joiner counts separately. The pinned reference does not establish whether the original service counts scalars, graphemes or UTF-16 units. No broader 1:1 parity claim is made.

## Implementation

Domain validation checks an independent 64,000-byte ceiling before scanning text, then the board's character limit, bounded globally to 16,000 scalars. Blank and unsupported-control checks remain. The shared nonrecursive formatter preserves the full accepted comment and caps independent oversized input at 16,000 scalars. Askama still escapes public and staff output.

Migration 0007 requires a UTF8 database, renames the board setting and its constraint, and replaces the post constraint with `char_length(comment) BETWEEN 1 AND 16000` and `octet_length(comment) <= 64000`. Existing numeric settings, text and grants are preserved. Store validation still reads the current setting while holding the board row lock.

Public URL-encoded forms allow 262,144 bytes, sufficient for a 192,000-byte percent-encoded maximum comment plus the bounded fields. Both `/post` and `/imgboard.php` use the same validation. Staff form limits remain 32 KiB and the JSON-only listener retains 64 KiB. The board help text says characters; the textarea has no UTF-16 `maxlength` that would reject valid astral text in the browser.

## Environment and commands

Verified September 8–9, 2026 in the Windows workspace using Rust 1.94.0, the locked dependencies, Playwright 1.62.0 with Chromium 151.0.7922.34, and PostgreSQL 16.15 in the disposable Ubuntu WSL cluster. Separate ignored `.local/database.ps1`, `.local/media.ps1` and `.local/staff.ps1` files supplied the test credentials. Native staff builds used the local Perl path through `OPENSSL_SRC_PERL`; no machine-wide toolchain or identity setting changed.

| Command | Actual outcome |
|---|---|
| `cargo run -p board-store --bin board-migrate --locked` | Applied migration 0007 to the disposable development database |
| `cargo test -p board-public --features database-tests --test comment_limits --test http_limits --locked` | Passed: seven HTTP tests, including a persisted maximum-size Unicode OP and reply |
| `cargo test -p board-public --test http_limits streamed_form_limit_applies_without_content_length --locked` | Passed: actual loopback HTTP with chunked transfer encoding and no Content-Length; exactly 262,144 bytes reached form validation (422), one byte over returned 413; security/cache headers retained |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo build --workspace --examples --bins --locked` | Passed |
| `cargo test --workspace --all-features --locked` | Passed: 84 tests, zero failed or ignored |
| `npm.cmd test` | Passed: five public behavior/API tests and three synthetic screenshot baselines |
| `npm.cmd run test:staff` | Passed: one real browser enrollment/login/moderation/recovery flow |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/test-comment-migration.sh` | Passed: actual migrations 0001–0006, historical Unicode/whitespace text and two different board settings, migration 0007, preserved values and grants, full 64,000-byte runtime-role insert, and an explicit LATIN1 encoding denial. Both generated databases removed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/test-staff-idle-migration.sh` | Passed: historical staff sessions retained authentication/absolute deadlines and activity defaults; generated database removed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` | Passed: post fingerprint and fourteen table counts matched; restored public/media/auth/staff reads, activity-column permissions and protected-operation denials passed. Generated database removed |
| `cargo audit` | Passed: 310 locked dependencies checked against 1,242 advisories |
| `npm.cmd audit --audit-level=moderate` | Passed: zero reported vulnerabilities |
| `.\.local\tools\actionlint.exe .github/workflows/ci.yml .github/workflows/advisories.yml` | Passed with actionlint 1.7.12 |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1: reference evidence, deployed media containment, production staff policy and deployed operations remain unverified |

## Regression and review evidence

The initial domain boundary run had two expected failures: the old byte check rejected multibyte text within the declared character limit, and the old formatter truncated a 16,000-emoji comment at 16,000 bytes. The new store test initially failed with PostgreSQL `42703` before the renamed column existed. The public form-budget test initially returned 413 instead of reaching form validation at 422. These failures were corrected in validation, parsing, the migration and the collector limit.

The first browser run passed all five behavior tests and the catalog screenshot. The two board screenshots failed as expected: desktop differed by 1,540 pixels and mobile by 822 pixels, confined to the help text changing from bytes to characters and its wrapping. Both actual screenshots and difference images were inspected. Only those two baselines were regenerated using `npm.cmd run test:visual -- --grep 'synthetic board' --update-snapshots`; the catalog baseline was retained.

The first migration exercise reached its final diagnostic check but failed because `rg` was absent in WSL. The script now uses the standard `grep` for that fixed-message check, and the whole exercise passed. A formatting check caught line wrapping after the byte-guard reorder; rustfmt corrected it before final verification. No test was ignored, weakened or replaced with a mock database.

The database-backed HTTP router regression checks `/boards.json`, board help, an OP whose maximum comment encodes to 192,000 bytes, full stored/HTML/JSON content, identical over-limit denials from both posting routes with unchanged ETag/reply count/post count, and a successful full-size reply through the legacy alias with a changed ETag. The JavaScript-disabled browser fills the advertised 4,000-scalar limit, reloads the full text, and verifies an over-limit reply leaves JSON and its ETag unchanged. Domain properties cover multibyte and combining text, scalar-safe truncation and approved link schemes. The staff template test retains the full 64,000-byte preview.

A separate source review found no Critical or Important findings. Its two Minor findings were addressed: the overflow test now sends unknown-length, multiple-chunk traffic over a real loopback listener, and the compatibility note uses explicit single-scalar examples instead of implying every emoji sequence counts as one. Final evidence and screenshot review are recorded here; hosted results will be recorded on the draft pull request.

## Deployment and remaining limits

Stop public and staff serving before migration 0007 and restart with compatible binaries after the operator migration. Review proxy request limits and keep the backup: old public binaries require the old column, and old renderers truncate longer comments. A binary-only rollback is insufficient. See [operations](operations.md) for rollout prerequisites and the [compatibility matrix](compatibility.md) for remaining exceptions.

Larger accepted comments increase possible thread, catalog and staff-queue response memory. Row and request caps do not establish a production response-memory budget. Mixed load, external CPU/memory/process enforcement and response-budget qualification remain open.

No worker was introduced or run, so this checkpoint provides no new evidence about worker filesystem, network, credential or resource containment. Public media enablement remains rejected. Actual isolated execution and external negative tests with healthy positive controls remain required by [issue #5](https://github.com/frankischilling/26chan/issues/5). Permitted visual/behavioral reference evidence remains unresolved in [issue #6](https://github.com/frankischilling/26chan/issues/6). Hardware authenticator and recovery policy, deployed identities/origins, backup isolation, monitoring and operational exercises also remain prerequisites. No production deployment or release was attempted.
