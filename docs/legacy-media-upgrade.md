# Legacy media manifest upgrade

`media-backfill` upgrades a legacy approved PNG whose MD5 and thumbnail manifest are NULL. It preserves the full PNG bytes, asset ID, media number, post text and consumed-upload record. It does not approve pending jobs, restore deleted files or replace existing manifests. This is an offline development operator command, not a public/staff route or an enabled production service.

## Procedure

Apply migration 0018 and build the command:

```text
cargo run -p board-store --bin board-migrate --locked
cargo build -p board-media-admin --bin media-backfill --locked
```

Stop competing publishers, backfill commands and cleanup before upgrading the canonical complete output store. Preserve a consistent database/file backup and keep readers stopped if the installation needs an all-at-once metadata transition. An individual upgrade is safe for running readers, but it does not make a cross-store backup atomic.

Use a fresh protected operator shell with `APP_ENV=development` and only the offline `MIGRATION_DATABASE_URL` credential. The URL must name `board_migrator` on loopback, with a password and no query/fragment. Public, staff, authentication, coordinator, reader, intake and monitoring credentials cause startup rejection. Never put this migration credential in a runtime service environment or dispatcher configuration. For the preprovisioned Linux setgid reader store, also set `MEDIA_GROUP_READ=true`.

```text
media-backfill CLIENT_CONFIG ABSOLUTE_APPROVED_STORE ABSOLUTE_QUARANTINE ASSET_ID
```

Both directories must already exist, must not be symlinks or overlap, and must belong to the reviewed storage layout. `CLIENT_CONFIG` is the existing private authenticated dispatcher configuration. `ASSET_ID` is one exact 32-character lowercase hexadecimal identifier selected by the operator. There is no directory scan, automatic batch selection or raw-file fallback. The command rejects production mode; a production deployment needs separate qualification and approval.

The command holds the same permanent publication lock used by promotion and cleanup. It reads at most the approved full-file size, capped at 5 MiB, and checks its SHA-256. It sends only those PNG bytes through the existing mutually authenticated dispatcher. The decoder still runs in the isolated job context without migration credentials. The dispatcher call has a 29-second timeout and the asynchronous command a 45-second deadline. This does not bound a stalled kernel filesystem operation; use the reviewed local filesystem and supervised operator execution.

Worker output must satisfy the fixed bounded RGBA protocol. Re-encoding those pixels must reproduce the approved PNG's exact SHA-256, length and dimensions before any thumbnail install or metadata mutation. A differently encoded legacy PNG can therefore fail even if its visible pixels are equivalent. The command leaves the original unchanged and reports failure; it never adds a privileged decoder or silently changes the original to make the comparison pass.

The command installs and syncs the missing thumbnail before committing the manifest. Migration 0018 permits only `board_migrator` to make the one-way transition from all-NULL to a complete manifest on an approved asset. Full-file identity and existing manifests remain immutable; runtime grants do not change. The Read Committed transaction follows board/thread/job/asset lock ordering, rechecks serving eligibility and updates thread modification time in the same transaction, invalidating date-based caches as well as changed-body ETags. A two-second database lock timeout fails the attempt without deleting possibly committed files.

An unattached legacy approval can still be consumed by public posting. Backfill locks its job before committing. If a post appeared during that lock wait, it releases the transaction and restarts with the newly discovered board/thread locks before the job lock. The durable one-use attachment link permits at most one such restart and avoids waiting for a board while holding a job needed by posting.

## Interruption and retry

Rejected original bytes or mismatched pixels change no output files or database metadata. An installation failure can leave a fixed staging file. Failure after installation but before a known commit may leave a fixed-name thumbnail whose NULL manifest keeps it unavailable to the reader. Keep these files and retry against the same canonical store. An identical retry verifies the existing bytes; conflicting output fails without overwriting it. A committed upgrade is detected on retry and both full/thumbnail files are verified without contacting the dispatcher.

Deletion or archive expiry during processing prevents the metadata commit and cannot restore serving authority. Normal retirement cleanup removes both representations. Never delete a thumbnail merely because a commit returned an error: the database commit may have succeeded. Unknown results require inspection or an idempotent retry. Corrupt originals, missing files, conflicting thumbnails, unavailable approvals, malformed output and changed pixels all fail closed.

## Verification

On the owned Windows PostgreSQL 16.15 cluster, migration 0018 applied and the focused database/TLS test and CLI rejection test passed:

```text
cargo test -p board-media-admin --features database-tests --test backfill --test backfill_cli --locked --jobs 1
cargo test -p board-media-admin --all-features --locked --jobs 1
cargo test -p board-store --features database-tests --test media_assets --test post_media --locked --jobs 1
cargo test -p board-config --locked --jobs 1
cargo test -p board-public --features database-tests --test legacy_media --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-media-admin -p board-store -p board-config -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
python scripts/test-attachment-restore.py
python -m py_compile tests/media/test_http_service.py
```

All eight media-admin tests, five selected store tests, 18 configuration tests, all 42 public tests and populated restore passed. The backfill database test checks runtime identity denials, original-file preservation, MD5 and thumbnail metadata, stable media numbers/text, thread modification, immutable existing manifests, pending-asset refusal, mismatched pixels, corrupt originals, conflicting thumbnails, death after thumbnail installation, and deletion during processing. Its actual CLI exchanges exact PNG bytes with a controlled mutually authenticated TLS server; its retry succeeds after that server stops. Credential-denial cases use that same healthy server and writable store, followed by successful processing. The public/API test checks changed ETags across thread/index/catalog, old-date GET/HEAD invalidation and preserved post/file fields. The public suite includes actual Chromium posting/deletion with JavaScript disabled. Initial clippy findings were corrected without suppressing warnings; the final all-target/all-feature run passed with warnings denied. Formatting and Python syntax checks also passed.

A controlled race uses the actual intake, queue and public posting credentials, with separate job/asset locks forcing posting to commit during backfill. The test verifies that the new thread receives the metadata-change timestamp. Temporarily disabling attachment rediscovery made that exact assertion fail; restoring the guard passed. No test bypass remains in the implementation.

The local TLS server supplies fixed synthetic RGBA output and is not decoder-containment evidence. `scripts/test-media-dispatch.sh --http` now adds a native upgrade case using the real gateway, Firecracker decoder, canonical store and separately identified HTTP reader. It checks exact original preservation, equivalent manifests, reader storage restrictions, retry and guest cleanup. The new native case awaits CI on the committed head. Production filesystem/secret/network policy, independent review and full original-site compatibility remain outside this local result.
