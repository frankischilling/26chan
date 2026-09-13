# Post attachment storage

This is the storage portion of [issue #48](https://github.com/frankischilling/26chan/issues/48). Public browser upload, processing-status forms, attachment rendering and attachment JSON fields are not connected yet. Existing public routes still serve text posts. This work does not enable production uploads or close the issue.

## Authorization and lifetime

`create_post_with_attachment` accepts an intake reservation and its bearer capability. It creates the post, its deletion-password record and the attachment in one transaction, alongside the existing thread creation, bump and reply-limit operations. `create_post` retains its text-only behavior.

The restricted `content.insert_post_attachment` function inserts a new post. It cannot add or replace a file on an existing post. It verifies the hash of the upload capability, checks the job's published state and an approved output, and consumes the job ID once. It chooses the approved asset itself; callers do not supply an asset ID, filename, byte count or dimensions. A later failure in the posting transaction rolls back the capability consumption too. Raw upload bytes are never a fallback.

Attachment authorization expires two hours after intake reservation. The function checks wall-clock expiry after acquiring the job lock. This deadline is project policy, not an observed original-site rule. Queue metadata can be retired independently after consumption; attachment metadata remains durable. Unused expired capabilities cannot attach files even if their outputs were approved.

`content.post_media` has a unique post, job and asset relation. Public credentials have neither direct table access nor approval authority. The callable functions belong to `board_attachment_owner`, a NOLOGIN role with column grants for the named operations. It has no processing-lease, staff-identity, deployment or schema-creation authority. Its membership is available only to the migrator for ownership transfer. Function search paths are fixed and PUBLIC execution is revoked within the migration transaction, following PostgreSQL's [function security guidance](https://www.postgresql.org/docs/16/sql-createfunction.html#SQL-CREATEFUNCTION-SECURITY).

All attachment creation takes the board, thread and job locks in that order. The function requires Read Committed so the image-count query sees commits that preceded acquisition of the board lock. The board's `image_limit` defaults to zero and permits at most 1,001 currently visible attachments per thread. File/post deletion frees an image slot but does not make a consumed capability reusable. These count and deadline rules are project-defined pending reference evidence.

## Deletion and reads

`delete_attachment` requires its caller to verify the post deletion password or staff authorization first, like the existing whole-post deletion store method. Its SQL function can set a file's tombstone but cannot restore it. It updates the thread modification time in the same transaction. The HTTP deletion control is not connected yet.

The public/staff metadata view excludes job IDs and capabilities. It returns a file-deleted marker for a tombstoned or unavailable approval, and hides posts belonging to deleted or expired threads. Filenames remain untrusted display text and must be escaped by the pending HTML renderer.

The separate media reader still has only its approved-asset view. That view now excludes attached files after file deletion, post deletion, thread removal or archive expiry. Existing opaque `/media/{id}.png` URLs therefore lose read authorization too. The HTTP reader already rechecks database authorization before conditional responses and requires cache revalidation. Previously downloaded copies cannot be recalled, and a read authorized before a concurrent deletion can finish.

Unattached approvals retain the existing private qualification behavior. Physical erasure of approved files, orphan retention, legacy media URLs, normalized-download metadata, thumbnails and the browser posting/status workflow remain unfinished. A tombstone currently removes serving authority, not the file from disk.

## Migration and checks

Fresh installations use `deploy/roles.sql`. Existing development installations need `deploy/attachment-role.sql` applied once by their database bootstrap administrator before migration 0012. Do not pass bootstrap credentials to a runtime. The migration adds a zero-default board policy, attachment table, two restricted functions and two reader views. Existing text data and media approvals are retained. Older public binaries continue text-only operation; do not enable image boards until the complete posting/read path is qualified.

With the existing private development database environment loaded:

```text
cargo run -p board-store --bin board-migrate --locked --jobs 1
cargo test -p board-store --features database-tests --test post_media --test media_intake --test media_assets --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-store --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo fmt --all --check
```

Migration 0012 and the three database suites passed locally on the owned Windows PostgreSQL 16.15 cluster on September 12, 2026. The attachment test uses actual public, intake, coordinator, reader and staff logins. It checks one-use and image-limit races, capability substitution/revocation/expiry, expiry during a real lock wait, failed-post rollback, media visibility after deletion, staff mutation races, archive expiry and preservation after queue cleanup. It removes its synthetic board, posts, attachments and jobs even when an assertion fails.

The existing public-app suite also passed: 35 tests covering text posting, deletion, JSON, snapshots, limits, CORS, archives and startup. A separate generated PostgreSQL database was migrated through 0011 and populated with the demo posts and a durable approval. Applying 0012 with `psql -c BEGIN -f migrations/0012_post_attachments.sql -c ROLLBACK` left no attachment objects and preserved the old records. Applying it with `--single-transaction` retained both posts and the approval, left all boards at an image limit of zero, and preserved reads through actual public and media-reader logins. The generated database was removed afterward; the source database was unchanged by that exercise.

These are storage and authorization tests. They do not execute a guest or decode files. The existing intake/Firecracker qualification remains separate; connecting that full path to browser posting is still required. Native Linux CI, a populated attachment restore, browser tests and visual review have not yet run for this branch.

The first broad store test attempt lacked the existing `test` and `limit` fixture boards. After applying `fixtures/demo.sql`, archive, comment, posting concurrency, media approval, intake and queue tests passed. The same full-suite command then stopped at the monitoring test's explicit Linux `/tmp/board-postgres.*` cluster requirement. The Windows cluster was not relabeled to bypass that check; the monitoring test remains enabled for its owned Linux CI environment. The attachment test was rerun separately and passed. The first attachment test build also caught a test-helper SQL string lifetime mismatch; using its actual static test statements fixed the build. Clippy, formatting and shell syntax checks passed.
