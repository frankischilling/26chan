# Image duplicate admission

Issue #213 includes the separate image-content checks in
`imgboard.php:6103–6144`. They run after ordinary actor cooldowns and before
Robot9000. VIP and staff badges exempt these checks; an ordinary Pass does not.
Active-post checks and upload-board history are wired to posting. Final browser
and CI qualification is still in progress.

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

## Upload-board history

The source also reads `SQLLOGMD5` with a strict one-day predicate. An entry at
exactly one day is outside the window. The supplied `/f/` and `/test/` settings
name `f_md5`; only an accepted `UPLOAD_BOARD` post writes it. `/test/` does not
enable `UPLOAD_BOARD` in the supplied configuration. `admin.php` removes rows
older than two days.

Migration 0242 records accepted upload-board attachments in a private history
namespace. A checked badge bypasses the lookup but still records an accepted
upload. A later transaction failure rolls the record back with the post and its
one-use attachment. Removing or archiving a post does not erase its recent-file
history. Read-enabled boards that are not upload boards do not record uploads.

The supplied `/f/` and `/test/` table label seeds the native `f_md5` namespace.
The source selected its physical database from an external `boardlist.db` value,
which was not supplied. Sharing that native namespace is a deployment assumption,
not proof that those source boards used the same database. An operator can assign
separate `image_history_scope` values and corresponding private scope rows when
the original board map is known. `README2.txt` also labels its keys and other
schema details as inferred; they do not establish original uniqueness behavior.

The scope lock serializes lookup and recording across boards sharing a namespace.
The default capacity is 100,000 retained rows per scope, configurable by the
migration owner up to 1,000,000. Insertion trims at most 256 records strictly older
than 48 hours. If capacity is still exhausted, the post fails without consuming
its attachment. Recent records are never discarded to make room. This bounded,
transactional failure is a backend safety difference from the source's unchecked
`INSERT IGNORE` result.

`board-image-history-retention run` removes up to 1,000 records per transaction
and 16 batches per invocation. Its server-owned cutoff is strictly older than
48 hours. It accepts no IDs, cutoff or duration, cannot read hashes, and requires
the existing dedicated `board_moderation_retention` login through
`MODERATION_RETENTION_DATABASE_URL`. The supplied `deploy/image-history-retention`
service and timer run it once per minute. Install the built binary alongside the
other retention tools and use the existing moderation-retention environment file.
These files do not deploy or enable a service by themselves.

## Qualification

The extractor pins the source gate and relevant configuration files by SHA-256.
The PHP harness executes the unchanged gate against owned rows in three constant
profiles. Its 288 cases cover badge dispatch, thread/opening-post scopes,
archived/deleted files, repost thresholds and the strict history boundary. Rust
compares its decision order with those executed results. The harness also executes
the accepted-upload recording and cleanup statements, including badge-independent
recording and the strict two-day cutoff.

Owned PostgreSQL tests exercise actual intake capabilities, normalized metadata,
accepted posts, refused retries, archive/file-deletion release and concurrent
writers. Migration checks include rollback and populated restore. Actual public
HTML and legacy JSON submissions return the duplicate link without consuming
the upload or adding a reply. Response tests also cover exact large IDs and
unsafe-link refusal. History tests cover retirement, expiry, capacity rollback,
read-only scopes, concurrent boards sharing a namespace, and verified-badge
recording with late-failure rollback. The
retention SQL checks bounded batches, private grants and authority drift.

```sh
python3 scripts/extract-image-duplicate-reference.py /path/to/4chan-old --check
php -n scripts/test-image-duplicate-source.php
cargo test -p board-domain image_duplicates --locked
cargo test -p board-store --features database-tests --test image_duplicates --locked
```
