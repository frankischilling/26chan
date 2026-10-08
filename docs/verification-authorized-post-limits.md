# Rank-specific posting limits

The supplied source gives moderators, managers and administrators larger raw
posting fields. Janitors keep ordinary limits. This change applies those limits
to the existing public staff posting route and private `/j/` discussion. Later
[staff identity work](verification-staff-identity.md) adds source badge choices,
name preparation and the administrator's forced-anonymous name exception.
Attachment posting and other privileged staff workflows remain unfinished.

## Source evidence

`scripts/extract-authorized-post-reference.py` checks the pinned `imgboard.php`,
`lib/auth.php` and public configuration hashes before extracting selected input
and line-check blocks. It executes only those bounded blocks and the pure rank
comparison, with synthetic ranks and an error sentinel. It never executes the
source application, sessions, private configuration or database code.

The retained fixture contains all 82 configured authorized comment budgets and
312 independent cases: Janitor, Mod, Manager and Admin on `/g/` and `/j/`.
Cases cover raw name/email/subject boundaries, multibyte fields, ASCII and
four-byte comments, newline normalization, repeated lines and board line caps.
Each PHP worker has a memory limit and deadline. `--check` compares canonical
UTF-8/LF fixture and migration bytes.

| Check | Janitor or ordinary posting | Moderator, manager or administrator |
| --- | --- | --- |
| Raw name, email and subject | 100 bytes | 255 bytes |
| Comment | Board's ordinary scalar budget | 10,000 scalars; `/j/` uses 50,000 |
| Newlines before the raw comment bound | CRLF and lone CR become LF | Same |
| Repeated-line and board line checks | Enforced | Source exemption |
| Finished escaped display name | At most 255 bytes | Same |

Cleanup still follows raw validation. An oversized field cannot become valid
because cleanup removes its characters. Blank replies without an attachment
remain invalid. Authorized subject tab expansion can occupy up to 1,020 saved
bytes; ordinary subjects retain their 400-byte ceiling.

## Storage and authorization

Migration 0069 adds `max_authorized_comment_chars` without changing ordinary
budgets. Migration 0070 adds a persisted post marker and extends the private
single-use proof with its selected limit and authorization flag. Existing posts
and pending proofs default to ordinary bounds.

The new issuer retains the existing account/session locks, recent-authentication
checks, board scope, revocation checks, capacity and private authorship rules.
It binds the final prepared body and saved wordfilter data in one statement.
Issuance and consumption require the current account rank and captured board
budget. The content trigger locks the board before consuming the proof. It
resets the marker for ordinary inserts; a forged transaction setting cannot
enlarge their SQL constraints.

The NOLOGIN proof owner receives only `slug`, the ordinary comment budget and
the authorized budget from board metadata, including `/j/`. Runtime roles
cannot change the board budget or post marker. Only the authentication role can
issue proofs, and only the staff-content role can consume them. Existing
function identities, owners, ACLs and fixed search paths are retained.

Larger filtered comments use the bounded `WF02` saved format. Its decoder keeps
the finite tag/entity grammar, UTF-8 checks, discriminator validation, framing
checks, part limits and trailing-byte rejection. Historical `WF01` rows retain
their format and formatter. Staff authorization also selects the larger markup
and saved-comment bound, so accepted text is not silently truncated at the old
16,000-scalar parser cap.

Selected staff rows receive a larger page-read allowance from their persisted
marker. Ordinary slots retain their former allowance. The overall board ceiling
still equals the former allowance for 1,000 selected threads, and the overall
thread read cap remains unchanged. Only the two staff posting routes accept
the larger 1 MiB form envelope; authentication and other staff routes keep
256 KiB.
Form maxlength values allow two UTF-16 units per admitted comment scalar. The
server still validates normalized scalar counts and raw UTF-8 field bytes.
The native snapshot validator admits staff badge names up to 255 UTF-8 bytes
and mobile subject titles up to 1,020 bytes. Mobile name titles retain the
finished 255-byte bound. Badge recipes, inert HTML grammar, aggregate response
budgets and worker deadlines remain enforced.

## Qualification

The twelve staff HTTP/database tests pass against real PostgreSQL runtime
roles. They verify all source board budgets, all four ranks, full 50,000-scalar
four-byte comments in forms larger than 600,000 bytes, one-over failures,
cleanup order, line exemptions, safe public JSON/previews, private-board
visibility, exact wordfilter data, and finished escaped-name rejection. An
unfiltered 50,000-scalar tab input expands to 199,994 characters and remains
complete in saved formatting, both JSON listeners, quote previews and reports.
Filtered and unfiltered authorized posts also produce bounded search excerpts.
Rejected writes leave post counts, audit records, clocks and proofs unchanged.
An owned synthetic stored-data fixture uses forty rows at the SQL body bound.
A smaller catalog page loads those rows, while the oversized whole catalog
returns `ReadLimit` before transferring comment bodies. This tests the saved
read ceiling independently of raw posting validation.

Two witnessed board-lock races change either rank or board budget while the
actual HTTP writer waits. Both reject the post after release; restored authority
passes the healthy control. Separate proof tests reject NULL and mismatched
limits, stale ranks/policy and replay. Legacy issuance still creates ordinary
rows. Actual runtime roles are denied marker/policy writes and unauthorized
proof issuance.

Fresh role bootstrap and the populated 0069/0070 upgrade pass. The upgrade
compares old post bodies/formats, thread clocks, operator settings, private
proof fields, function metadata and content ACLs in both directions. It
consumes a preserved ordinary proof, accepts a new authorized proof, and
rejects forged public settings at the original ordinary SQL bounds.

The final workspace run passes 690 tests across 172 groups, with no failures
or ignored tests. Formatting and strict Clippy pass for the same code. The
native Windows domain run passes 143 tests in 18 groups, also with no failures
or ignored tests. All 138 affected public browser cases pass, including the
corrected native emoji-name case, live updater/tail behavior, quote previews,
search and filters. The 107 targeted JavaScript and isolated DOM tests pass,
and all six generated bundle checks pass. The separate staff browser flow
passes, including script-disabled larger posting and the later administrator
reply arriving through the live updater.

The pushed checkpoint `2c44fb5c5f411fb1d3b069cff9f4cf96a98bd2f5` passes
[build/test 37087130371](https://github.com/frankischilling/26chan/actions/runs/37087130371)
in full, including Linux Rust/browser checks, every Windows visual group,
media/operations and the aggregate gate. Its
[monitoring run](https://github.com/frankischilling/26chan/actions/runs/37087130389)
and [dependency advisory run](https://github.com/frankischilling/26chan/actions/runs/37087130377)
also pass. Later staff identity work requires its own qualification.

## Retained failures and corrections

Larger accepted comments exposed the old 16,000-scalar markup/parser ceiling
and the ordinary page-read allowance. The formatter now selects its finite
bound from saved post authority, and page selection counts only authorized
rows in its read allowance. The public search query also needed the new marker
in its explicit column list; its ordinary source-wordfilter regressions and
new authorized excerpts pass after that correction.

The first populated policy attempt changed a forged missing-board proof's SQL
error from `28000` to a foreign-key error. The content trigger now retains the
authorization error. Qualification uses a separate freshly migrated database;
the first database and applied checksums remain unchanged.

A later workspace/browser attempt stopped at database connection setup when
the WSL filesystem filled with compiler cache. With no compiler running, only
the owned incremental build cache was removed. The same PostgreSQL cluster was
restarted, its migration ledger was checked against canonical files, and the
qualification was resumed. Database data, source, built binaries and failure
logs were preserved. No application limiter or test deadline was enlarged.

One subsequent local wrapper omitted `PLAYWRIGHT_BROWSERS_PATH`, so the media
browser could not find the already installed Chromium. Restoring that existing
path makes the actual-reader browser control pass. No browser or assertion was
changed for that environment correction.

The staff browser then saved the larger moderator post but rejected its
snapshot when updating for a later administrator reply. The badge validator
still had a 100-byte name ceiling. A new regression reproduces that rejection
and checks the 255-byte name and 1,020-byte subject bounds in updater, preview,
board-page and live-tree parsing. Multibyte one-over values remain rejected.
The mobile tooltip validator uses the corresponding saved field bounds. Both
fixed parser bundles are regenerated from their pinned dependencies; no
deadline, retry, screenshot tolerance or badge grammar was loosened.

The prior hosted emoji-name failure is recorded in
[CI fixture verification](verification-ci-board-fixtures.md). The browser
follow-up retains the 100-byte raw input and both one-byte-over denials and
requires the source's saved `Anonymous` result.

## Deployment and rollback

Stop staff writers before migration, apply the additive migrations as the
migration identity, then start binaries that understand the new schema and
`WF02`. Keep the prior database backup and migration checksums unchanged.
Older binaries do not decode `WF02` and show an inert unavailable-comment
representation. A rollback that must render new authorized rows therefore
needs a compatible decoder, or a restore from before those rows were written.
Do not relabel new frames as `WF01`, truncate saved bodies or rewrite old
migration checksums.

With the owned role environments loaded:

```sh
python3 scripts/extract-authorized-post-reference.py 4chan-old crates/domain/tests/fixtures/authorized-post.json --migration migrations/0069_authorized_comment_policy.sql --check
sudo bash scripts/test-authorized-post-migration.sh
sudo bash scripts/test-role-bootstrap.sh
cargo test -p board-domain --locked
cargo test -p board-staff --features database-tests --test posting --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
node node_modules/@playwright/test/cli.js test --config playwright.staff.config.js
```
