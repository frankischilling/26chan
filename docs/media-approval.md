# Durable media approval

This slice connects bounded, stopped-guest output to durable approval records. It is an operator development workflow. Public uploads and media serving stay disabled until authenticated dispatch and the remaining deployment boundary checks are qualified.

The publication authority uses `board_media` and a private output directory. Each lease gets a separate random output ID; neither the job ID nor its lease token becomes the public identifier. A pending record reserves the exact host-encoded PNG digest, length and dimensions. The publisher installs complete bytes, syncs them, and then approves the record in the same database transaction that completes the still-current, unexpired job. A duplicate with identical metadata reuses the reservation; different metadata is rejected. Approval survives deletion of terminal queue metadata.

All publishers and cleanup operations for a store take one permanent `.publication.lock` using a nonblocking operating-system file lock. They acquire it before checking a lease and retain it until approval or failure. This deliberately serializes publication for the existing bounded processing profile. It requires a local filesystem with supported locks and atomic hard links, and an operator-owned directory that workers and readers cannot write. The lock is never unlinked. It is not protection against a compromised publication authority or an operator replacing the directory.

Configure exactly one canonical output root for each database, shared by every publication and cleanup invocation. The database does not bind a row to a filesystem path. Changing that mapping or using two roots concurrently violates the recovery contract and can leave untracked files; stop operations and reconcile before an operator-managed storage migration. The development commands accept paths from the operator, so deployment configuration must enforce this mapping.

Interrupted publication leaves private, unapproved bytes. Cleanup uses the same storage lock, selects at most 64 abandoned reservations, rechecks each lease, marks the record deleting, removes only that generated object's fixed filenames, syncs the directory, and then removes the deleting record. Retrying cleanup is safe after interruption at either boundary. Active leases and approved records are ineligible. Each attempt uses its reserved ID for its staging file, so recovery needs no directory scan. Approved records and their metadata are immutable to the runtime role.

The `board_media_read` role has SELECT on a security-barrier view containing only approved IDs and their digest, length and dimensions. It has no access to jobs, tokens, pending records, content, staff identity, deployment data or writes. Reading first requires a record from this view and then checks the exact regular file's bounded bytes and digest without decoding it. A database failure denies the read. The directory must never be exposed by a static web server: the filesystem link can precede approval.

The publication store syncs its directory on Unix. Windows supports development behavior tests but this slice does not claim power-loss durability on Windows. Actual storage and database power-loss testing, a separately deployed HTTP media origin, authenticated intake/dispatch and post-to-asset attachment remain required before public enablement. No raw input is published.

References: PostgreSQL's [view security rules](https://www.postgresql.org/docs/16/sql-createview.html) describe the barrier and owner-based table privileges; Rust's [file API](https://doc.rust-lang.org/std/fs/struct.File.html) documents nonblocking file locks and synchronization. The workspace remains pinned to Rust 1.94.0 and its existing dependency lockfile.

## Operator development commands

Create `board_media_read` with `sudo bash scripts/dev-media-reader-db.sh` before applying migration 0008. Reuse existing `.local` credentials if already provisioned. The migration preserves legacy job receipts and deliberately creates no approvals for old files. Those files need explicit validation and reconciliation; a receipt alone never grants access. The `media.assets` table grows independently of queue retention, so a production asset-retention/deletion policy is still needed.

Build `cargo build -p board-media-admin --bins --locked`. Run publication commands in a fresh shell containing only the writer credential:

```bash
source .local/media.env
export APP_ENV=development
export MEDIA_QUARANTINE_DIR="$PWD/.local/quarantine"
target/debug/media-publish claim .local/lease.json
# Operator supplies this job's quarantined input to the isolated runner in a
# separate credential-free invocation, then collects its stopped output disk.
target/debug/media-publish publish .local/lease.json .local/stopped-output.disk "$PWD/.local/objects"
target/debug/media-publish reconcile "$PWD/.local/objects"
```

The lease manifest must be a new private file; claim prints no token. It contains `job_id` and `lease_token`, is capped at 512 bytes, rejects extra fields, and uses mode 0600 on Unix. Its directory and Windows ACL must be private to the operator. The lease lasts 30 seconds; an expired attempt is rejected. Delete the manifest when no longer needed. An interrupted claim can leave an empty file and a lease that expires normally. Reconciliation handles one batch of at most 64 abandoned outputs; run again while it removes a full batch. Queue expiration and terminal input cleanup remain separate `board-media-admin cleanup` operations.

Use a different fresh shell containing only the reader credential:

```bash
source .local/media-reader.env
export APP_ENV=development
target/debug/media-read OUTPUT_ID "$PWD/.local/objects" .local/approved-export.png
```

The export destination must not exist. Readers create no storage entries and fail closed on unavailable approval state, unknown IDs, nonregular files, incorrect lengths or digest mismatch. Both commands reject inherited application/operator credentials they do not need. They provide no HTTP route or cookie boundary.

Development media database URLs must have the expected login and a loopback host, with no query string or fragment. SQLx can interpret URL options separately from the authority; rejecting options keeps the checked host/login authoritative before connection. This restriction applies to both writer and reader commands.

For a disposable native Linux host, `sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-ci/decode.json bash scripts/test-media-publication.sh` exercises intake, claim, an actual isolated job, publication and restricted reads before and after queue deletion. It clears credentials from the root runner's environment. This is an operator-mediated qualification harness, not an authenticated service dispatcher.
