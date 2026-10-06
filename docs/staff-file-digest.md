# Staff file digest

The protected report view offers a read-only `File MD5` disclosure for available attachments on archived threads with a known normalized manifest. The value is 32 lowercase hexadecimal characters. It identifies the approved normalized file bytes, not the original upload. Public attachment JSON continues to serialize the same normalized digest as base64.

This is a bounded part of the original staff workflow. It does not reproduce the full original popup UI or its other actions. The source-original cleanup-before-MD5 behavior and JPEG thumbnail output remain a documented compatibility exception, [E-008](source-media-digests.md). This feature does not claim original-upload checksum parity. MD5 is display metadata, not a security or authorization primitive.

## Database boundary

Migration `0107_staff_file_digest.sql` uses `CREATE OR REPLACE VIEW` to append nullable `md5` to `content.staff_post_media`. The original ten columns, their order and types, owner, grants and security barrier remain unchanged. No backfill or historical row rewrite is needed. Staff still cannot read `media.assets` or `content.post_media` directly; public and authentication roles cannot read the staff view.

The digest is present only when the asset is approved, the post and attachment remain available, the thread is visible and any archive deadline has not passed. Removed files, deleted posts or threads, expired archives, pending or deleting assets, missing assets and legacy NULL manifests yield NULL. The SQL projection accepts only a 32-byte lowercase hexadecimal digest.

The new digest guard checks archive expiry with `clock_timestamp()`. The original `available` column retains its transaction-time semantics through `content.visible_threads`. In a transaction that began before archive expiry, `available` can therefore remain true after the deadline while `md5` has already become NULL. This distinction avoids changing the old view contract while withholding newly disclosed metadata after expiry.

## Qualification

Run the migration qualification as root on an owned disposable host with PostgreSQL 16:

```sh
sudo bash scripts/test-staff-file-digest-migration.sh
```

The script creates a private Unix-socket-only cluster, installs the deployment roles, and stops migration deployment at 0106. It applies only 0107, so later migrations cannot change this historical boundary. It exercises both an empty installation populated after the migration and a populated 0106 upgrade. Fixtures use ten synthetic attachments and keep database triggers enabled. Original posting registration runs as `board_migrator` within its transaction.

The checks cover retained table rows, the original ten view columns and read values, view owner and normalized grants, the security barrier, nullable appended text, actual staff/public/auth role reads, missing and unavailable digest cases, and archive expiry within an already-open transaction. An administrator dump/restore must preserve normalized metadata and read behavior. Fingerprints use role and object names, normalized ACL entries and column metadata without database names; they do not compare physical OIDs or `table_catalog` across databases. Temporary databases, fixtures, dumps and private diagnostics are removed with the cluster.

The adjacent CI step runs the same script. Local fresh/populated upgrades and administrator dump/restore passed using a loopback-TCP adapter for this executor. The original Unix-socket wrapper remains a hosted check. Both `table_catalog` and `udt_catalog` are normalized across restore; type schema/name, dimensions, nullability, grants and other metadata remain compared.

Local Linux qualification passed 480 selected Rust cases: 213 store, 126 public and 141 staff, with three Unix-socket exclusions retained in CI. Formatting and strict all-target workspace Clippy passed. Focused coverage passed 33 staff library, three attachment, four digest and eight snapshot cases, including deterministic wall-clock expiry. These are synthetic metadata tests, not new decoder/publication or original-file byte-equivalence evidence. Exact-head hosted CI and browser checks for this continuation remain pending.
