# Post attachments

This draft implements storage and a development browser workflow for [issue #48](https://github.com/frankischilling/26chan/issues/48). A user can upload a file, check processing status, submit an approved attachment with a post, and delete just the file. Thread, board and catalog HTML show approved attachments from the separate reader origin. JSON metadata, real thumbnail variants and legacy media URLs remain unfinished. This work does not enable production uploads or close the issue.

## Browser workflow and configuration

The image form sends `resto` followed by one `upfile` multipart field to `/{board}/upload`. The public process streams at most 8 MiB through an authenticated connection to the intake service. It neither decodes the file nor reads quarantine storage. The route allows 16 KiB of multipart overhead, admits four uploads, and retains the public request's ten-second deadline. A one-slot channel forwards copied chunks of at most 16 KiB. The fixed-endpoint client caps response headers at 16 KiB and JSON bodies at 4 KiB; invalid responses and redirects fail closed.

The private, no-store response holds the upload capability in hidden form fields. Status checks and cancellation use POST, never secret-bearing URLs. No cookie or server-side draft is created. The user enters their comment and deletion password only after approval, then submits the normal post handler. A consumed, revoked or two-hour-old capability cannot open another ready-to-post form. Posting repeats authorization inside its transaction.

This two-step flow is a security-driven compatibility exception: processing must finish before the application commits an attachment, and credentials/drafts are not retained while the worker runs. It requires an extra status check and keeping the page open. There is no claim that it matches an observed original posting form. Core posting does not require site JavaScript.

Unexpected trailing multipart fields are rejected after the bounded stream completes. The public caller revokes attachment authority on transport/parser failure when it can still finish its handler. An outer deadline or disconnect can interrupt that cleanup; unused reservations retain their bounded expiry and cleanup policy. A canceled running job may finish, but its revoked capability cannot attach the output. Physical orphan erasure remains unfinished.

Only an explicitly selected loopback development profile accepts uploads:

```text
APP_ENV=development
MEDIA_ENABLED=true
PUBLIC_MEDIA_PROFILE=isolated-development
PUBLIC_INTAKE_ADDR=127.0.0.1:<private-intake-port>
PUBLIC_INTAKE_TOKEN=<same-private-64-lowercase-hex-token-as-the-intake-service>
MEDIA_ORIGIN=http://127.0.0.1:<separate-reader-port>
```

Use the normal public `DATABASE_URL` and distinct public/staff/media origins. Do not inherit intake, coordinator, reader, migration or staff database credentials. Configure the board's image limit through the operator's database connection. Intake and reader services retain their own development settings and distinct credentials; only the public process receives this profile. The isolated dispatcher must run separately. Public readiness checks its database and authenticated intake; it does not qualify the guest, promotion store or deployed network boundary. Production still rejects `MEDIA_ENABLED=true`, and existing boards default to zero images.

## Authorization and lifetime

`create_post_with_attachment` accepts an intake reservation and its bearer capability. It creates the post, its deletion-password record and the attachment in one transaction, alongside the existing thread creation, bump and reply-limit operations. `create_post` retains its text-only behavior.

The restricted `content.insert_post_attachment` function inserts a new post. It cannot add or replace a file on an existing post. It verifies the hash of the upload capability, checks the job's published state and an approved output, and consumes the job ID once. It chooses the approved asset itself; callers do not supply an asset ID, filename, byte count or dimensions. A later failure in the posting transaction rolls back the capability consumption too. Raw upload bytes are never a fallback.

Attachment authorization expires two hours after intake reservation. The function checks wall-clock expiry after acquiring the job lock. This deadline is project policy, not an observed original-site rule. Queue metadata can be retired independently after consumption; attachment metadata remains durable. Unused expired capabilities cannot attach files even if their outputs were approved.

`content.post_media` has a unique post, job and asset relation. Public credentials have neither direct table access nor approval authority. The callable functions belong to `board_attachment_owner`, a NOLOGIN role with column grants for the named operations. It has no processing-lease, staff-identity, deployment or schema-creation authority. Its membership is available only to the migrator for ownership transfer. Function search paths are fixed and PUBLIC execution is revoked within the migration transaction, following PostgreSQL's [function security guidance](https://www.postgresql.org/docs/16/sql-createfunction.html#SQL-CREATEFUNCTION-SECURITY).

All attachment creation takes the board, thread and job locks in that order. The function requires Read Committed so the image-count query sees commits that preceded acquisition of the board lock. The board's `image_limit` defaults to zero and permits at most 1,001 currently visible attachments per thread. File/post deletion frees an image slot but does not make a consumed capability reusable. These count and deadline rules are project-defined pending reference evidence.

## Deletion and reads

`delete_attachment` requires its caller to verify the post deletion password or staff authorization first, like the existing whole-post deletion store method. Its SQL function can set a file's tombstone but cannot restore it. It updates the thread modification time in the same transaction. The public file-only checkbox uses the existing deletion-password check; staff HTTP file-only controls remain unfinished.

The public/staff metadata view excludes job IDs and capabilities. It returns a file-deleted marker for a tombstoned or unavailable approval, and hides posts belonging to deleted or expired threads. The HTML renderer escapes filenames and labels downloads as normalized PNGs. Nonspoiler images use the complete approved PNG with CSS size bounds; these are not separate thumbnail files. Spoilers display an explicit link without automatically fetching the image. Attachment reads share the existing board/thread snapshot transaction.

The separate media reader still has only its approved-asset view. That view now excludes attached files after file deletion, post deletion, thread removal or archive expiry. Existing opaque `/media/{id}.png` URLs therefore lose read authorization too. The HTTP reader already rechecks database authorization before conditional responses and requires cache revalidation. Previously downloaded copies cannot be recalled, and a read authorized before a concurrent deletion can finish.

Unattached approvals retain the existing private qualification behavior. Physical erasure of approved files, orphan retention, legacy media URLs, normalized-download API metadata and real thumbnails remain unfinished. A tombstone currently removes serving authority, not the file from disk. JSON still exposes the text-only subset and is not an accurate contract for development image boards yet; the draft cannot be merged as a completed attachment feature in this state.

## Migration and checks

Fresh installations use `deploy/roles.sql`. Existing development installations need `deploy/attachment-role.sql` applied once by their database bootstrap administrator before migration 0012. Do not pass bootstrap credentials to a runtime. Migration 0012 adds a zero-default board policy, attachment table, two restricted functions and two reader views. Migration 0013 adds capability checks and cancellation with no direct handle access for public credentials. Cancellation requires Read Committed and shares the posting job lock, so it cannot race a successful consumption and also succeed. Existing text data and media approvals are retained. Older public binaries continue text-only operation; image boards remain a qualification profile until the whole read/write contract is complete.

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

The storage checkpoint's Linux Rust/PostgreSQL/Firecracker workflow and Windows visual checks passed on commit `5e57aa3`. Those results do not cover the subsequent browser changes. A fresh owned Windows PostgreSQL database was migrated through 0013 without changing the previous database's recorded migration checksums. The updated attachment database test and `uploads` HTTP integration test passed, including cancellation/consumption races, expired status, 512 KB streaming, invalid/oversized forms and file-only deletion.

`cargo test -p board-public --all-features --test upload_browser --locked --jobs 1` passed locally. It runs the actual public binary, real intake and media HTTP listeners, restricted database logins and Chromium with site JavaScript disabled. A trusted test helper supplies synthetic bounded pixels through the normal publisher; it does not decode the uploaded file or execute a guest. The browser verifies separate-origin PNG display and a 404 for a conditional media read after deletion. This distinguishes the browser test from isolation evidence.

The Linux intake qualification now starts a separate nonroot public unit and runs the same browser script against real Firecracker dispatch. CI must execute this addition before it can be cited as passing. A populated attachment restore, complete API contract and regression baselines for full-size images/thumbnails remain required. Initial compilation caught a test SQL string lifetime and a missing test-only Tokio process feature; both were corrected before their tests passed.

The complete public suite passed all 41 tests. Its first run exposed a disabled-mode route regression (415 instead of the established 405); conditional route registration fixed it without changing the assertion. Configuration tests, focused clippy for public/config/store/intake, formatting, JavaScript/Python/shell syntax and cargo audit passed. Including staff in the Windows clippy command stopped at the vendored OpenSSL build because Perl was not on that command's path; this is not reported as a passing staff check. Linux CI retains the staff build and tests.

Five new screens (pending upload, approved post form, attached thread, catalog and file deletion) were captured and inspected at 1280×900 and 390×844 in local Chromium. The mobile checks found no horizontal overflow. Review caught and corrected the text-only footer in media mode. These synthetic one-pixel images check basic layout, not original-site visual parity or finished thumbnail presentation. Existing reference baselines were not refreshed.

The first broad store test attempt lacked the existing `test` and `limit` fixture boards. After applying `fixtures/demo.sql`, archive, comment, posting concurrency, media approval, intake and queue tests passed. The same full-suite command then stopped at the monitoring test's explicit Linux `/tmp/board-postgres.*` cluster requirement. The Windows cluster was not relabeled to bypass that check; the monitoring test remains enabled for its owned Linux CI environment. The attachment test was rerun separately and passed. The first attachment test build also caught a test-helper SQL string lifetime mismatch; using its actual static test statements fixed the build. Clippy, formatting and shell syntax checks passed.
