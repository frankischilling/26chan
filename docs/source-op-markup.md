# Source OP markup

The supplied `imgboard.php:566-574` applies `[b]`, `[i]`, `[red]`, `[green]`
and `[blue]` in that order, using `parse_bbcode_one` with one emitted nesting
level per kind. Parsing is case-sensitive, balances an unfinished opening tag
and retains the source's treatment of orphan and nested markers. The pass runs
after SJIS, spoiler and code processing and before final blank-content admission
(`imgboard.php:5785-5787`). The finite tags render as spans with the source
`mu-s`, `mu-i`, `mu-r`, `mu-g` and `mu-b` classes. Their CSS comes from
`yotsubanew.css:120-124`.

`OP_MARKUP` defaults off in `config/global_config.ini:609`. Only the active
`qst.config.ini:34` and `test.config.ini:52` enable it. Migration 0034 adds the
operator-owned setting, enables those two existing board names and leaves
other and future boards disabled. The synthetic seed applies the same two
overrides when creating boards after migrations.

## Ownership and storage

The source permits the OP itself and replies whose address **or** hashed
deletion password matches the OP (`imgboard.php:9573-9587`). Matching a
password does not make the reply an address-based self-bump. That distinction
is preserved in the separate bump records.

The public handler uses the actual socket address, including canonical IPv4
mapping; forwarded headers cannot supply identity. It verifies a reply's
password against the existing OP hash inside the shared hash semaphore. The
accepted Argon2id profile has fixed memory, iteration, parallelism, version and
output bounds. Missing or unsupported hash state provides no password proof.
Each new post still receives its own freshly salted deletion hash.

The server-owned posting context carries a fingerprint of the hash that was
verified. The store rechecks it under the board lock, so an operator's earlier
hash replacement cannot turn stale verification into markup eligibility. This
check also handles enabling the policy while a request waits. The context is
not deserialized from HTTP fields, stored in posts, returned in JSON or logged.

The writer supplies cosmetic reply eligibility through transaction-local
`board.source_op_reply`. The insertion trigger independently locks the board,
checks its operator setting and stamps the post. This setting is not staff or
deletion authorization. The public SQL writer is trusted to supply cosmetic
reply eligibility; it still cannot change board policy or rewrite saved format
stamps. `SET LOCAL` prevents the hint from leaking through pooled connections
after commit or rollback. The attachment inserter needs only an additional
read grant for the non-private board flag, with no new write authority.

Format zero and formats 8–15 retain their historical meanings. Formats 24–31
add the OP-markup bit to the existing three flags. Earlier rows are not
backfilled, and changing board policy does not reinterpret their comments.
Public HTML, both JSON routers, catalog text, updater fragments and staff
previews use the saved format. User text remains escaped; no arbitrary HTML,
style attribute, tag or class becomes valid. The updater accepts these five
classes only on a span with one class.

## Upgrade and rollback

Apply migration 0034 before the new binary. It is additive and preserves
historical post data and clocks. Keep the column, expanded constraint and
trigger when rolling the binary back. Earlier binaries treat formats 24–31 as
unknown and show bounded escaped text, including literal markup markers.
Removing the migration would require a separate data migration and would lose
the saved rendering policy.

The store reuses the workspace's already-locked SHA-256 implementation to bind
proof to the verified hash. The later Rustls 0.23.45 security update is recorded
in [dependency notes](dependencies.md); media-parser boundaries are unchanged.
Runtime password hashes and fingerprints remain private application
state. Media workers receive no new credentials, connectivity or authority.

## Verification

Domain tests cover all five passes, nesting, orphan markers, crossing tags,
interaction with code, all persisted format values and bounded randomized
comparison with the source byte-offset algorithm. Public and staff template
cases check finite markup with hostile text. Updater tests retain the existing
tree, byte, URL and attribute restrictions while checking the five new spans.

The actual HTTP/database test exercises both aliases, both encodings and both
response modes with matching addresses, matching passwords, neither match,
missing addresses, mapped IPv4, forged forwarded headers and missing password
state. It observes real board-lock waits, checks preserved historical output,
pool-context reset, separate self-bump records and denied policy/stamp writes.
Additional cases cover stale password proof and forged HTTP context fields.
The attachment test uses an approved file through the scoped inserter.

The populated upgrade test checks all 82 source board names, unchanged
historical fields and clocks, global defaults, runtime reads, denied writes
and OP/reply stamps. Desktop and mobile fixtures check all six theme variants;
the captured yotsuba pages were inspected. Persisted browser tests cover native
posting without JavaScript and live Quick Reply insertion with JavaScript.
Current-head hosted qualification is required before merge. These checks do
not establish complete formatting parity or production readiness; word filters,
linkification, wrapping and other work in #143 remain separate.

Local validation passed the full domain suite, all 46 public library tests,
the actual OP HTTP/database test, the attachment authorization suite, 16 updater
Node tests, both persisted browser modes and six theme fixture tests at two
widths. Strict workspace Clippy passed with all targets and features. The four
unchanged SQL blocks from the populated upgrade harness passed in a separate
owned native PostgreSQL database, which was removed afterward. Hosted Linux
still needs to run the complete guarded shell harness before merge.
