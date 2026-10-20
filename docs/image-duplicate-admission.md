# Image duplicate admission

Issue #213 includes the separate image-content checks in
`imgboard.php:6103–6144`. They run after ordinary actor cooldowns and before
Robot9000. VIP and staff badges exempt these checks; an ordinary Pass does not.
This implementation is in progress. The active-post checks are wired to posting;
the upload-board history check is still missing.

## Active posts

A reply checks the selected thread's opening post and replies. A new thread
checks other opening posts on the same board. Matching uses the approved
normalized file's MD5. Deleted posts, deleted files and archived threads do not
occupy this duplicate scope.

If the first check allows a reply, the source can also limit matching live
replies across its board. The supplied global limit is zero; `/wsg/` sets it to
three. Migration 0241 adds `image_repost_limit` with the same defaults. Equality
rejects: a board with three matching live replies cannot accept a fourth when
its limit is three. Opening posts do not enter this count.

The protected lookup holds the board, target and upload locks. It returns a
public post/thread pair or the board-wide limit refusal. Concurrent writers
serialize on the board, so two otherwise eligible identical uploads cannot both
pass the same scope. A refusal does not consume the upload capability. Archive
or file-only deletion can make a later retry eligible.

The source's `LIMIT 1` query has no ordering clause. The native query picks the
lowest matching live post ID for a stable link; this does not claim that the
original database always picked that row.

## Identity and output

The gate uses capability-scoped approved media metadata. Client-supplied hashes,
file names, a Pass presentation cookie or an unchecked badge flag cannot replace
that proof. VIP exemptions use the private posting receipt. Ordinary staff
posting keeps the duplicate check; selected staff badges still need the final
staff posting authority.

A new upload without a known normalized digest fails closed. Historical unknown
digests remain unknown; this migration does not manufacture provenance from a
filename or job ID. Synthetic transport tests provide explicit manifest digests
and do not claim to decode real image bytes.

Native HTML and JSON responses use the source duplicate wording. The optional
`here` link is built from a checked board and positive 64-bit post/thread IDs.
Quick Reply recognizes only that exact local-link form and creates DOM nodes;
it does not parse error HTML. Invalid paths, foreign origins, oversized IDs and
added markup remain plain text.

## Remaining upload-board history

The source also reads `SQLLOGMD5` with a strict one-day predicate. An entry at
exactly one day is outside the window. The supplied `/f/` and `/test/` settings
name `f_md5`; only an accepted `UPLOAD_BOARD` post writes it. `/test/` does not
enable `UPLOAD_BOARD` in the supplied configuration. `admin.php` removes rows
older than two days.

That persistent history path, its bounded retention and accepted-post recording
are not implemented by migration 0241. The schema in `README2.txt` labels its
keys and other details as inferred, so those declarations are not proof of the
original uniqueness behavior.

## Qualification

The extractor pins the source gate and relevant configuration files by SHA-256.
The PHP harness executes the unchanged gate against owned rows in three constant
profiles. Its 288 cases cover badge dispatch, thread/opening-post scopes,
archived/deleted files, repost thresholds and the strict history boundary. Rust
compares its decision order with those executed results.

Owned PostgreSQL tests exercise actual intake capabilities, normalized metadata,
accepted posts, refused retries, archive/file-deletion release and concurrent
writers. Migration checks include rollback and populated restore. Actual public
HTML and legacy JSON submissions return the duplicate link without consuming
the upload or adding a reply. Response tests also cover exact large IDs and
unsafe-link refusal. The source history cases qualify
the decision model only, not a persistent implementation.

```sh
python3 scripts/extract-image-duplicate-reference.py /path/to/4chan-old --check
php -n scripts/test-image-duplicate-source.php
cargo test -p board-domain image_duplicates --locked
cargo test -p board-store --features database-tests --test image_duplicates --locked
```
