# Attachment restore exercise

`scripts/test-attachment-restore.py` checks a PostgreSQL dump together with normalized image files. It creates two new, randomly named databases in the verified owned PostgreSQL 16 cluster. It never restores over the development database or changes its connection marker.

The source starts empty and receives current migrations. The Rust `attachment_restore` example uses the actual intake, publisher and posting stores with restricted logins to create seven synthetic attachments: a live OP, a spoiler reply, a deleted file, a retired file with cleanup interrupted before unlinking, a retained archive, an expired archive, and a deleted reply. Each has a normalized 500×300 PNG and separate 250×150 PNG thumbnail. The fixture supplies validated synthetic RGBA pixels; it does not decode uploads or execute a guest.

After the writer exits, the harness takes a custom-format `pg_dump` and copies the fourteen generated files into a private backup directory. The source remains quiescent. The bootstrap administrator restores the dump into the second empty database with `pg_restore --single-transaction --exit-on-error`, preserving ownership and ACLs. A restored publisher creates a new lock; the source lock is not copied.

Runtime roles receive no restore, migration or role-management authority. PostgreSQL documents that a database dump excludes cluster-wide roles and that restoration can execute source-controlled SQL. Only the freshly created, trusted fixture database is restored here. See [pg_dump](https://www.postgresql.org/docs/16/app-pgdump.html) and [pg_restore](https://www.postgresql.org/docs/16/app-pgrestore.html).

## Checks and actual result

The exercise passed three times on the owned Windows PostgreSQL 16.15 cluster on September 12, 2026 (America/New_York). The last two runs included missing-file and same-length corruption controls, followed by successful verification after repair. A separate refusal check confirmed that the fixture rejects the ordinary development database before creating files or connecting to it. All role URLs must name the same generated source or restore database.

- Exact fingerprints match for posts, threads, attachments, the media counter, assets, jobs, intake capability records and migrations.
- Public thread JSON matches for the active and retained-archive threads, including normalized metadata and deletion markers.
- Full/thumbnail bytes match through actual reader handlers at all four opaque/numeric URL shapes. Deleted, retired, expired and removed attachments stay unreadable while their restored files are present.
- Public access to raw attachment tables, staff credentials and retirement remains denied. Consumed receipts cannot attach again.
- Reconciliation resumes interrupted cleanup, removes the four eligible pairs, and preserves live files and all seven durable attachment records.
- New posting advances the post-number sequence and media counter. The counter fixture is ahead of wall clock, so an accidental reset cannot pass merely because time advanced.
- Restored cleanup and posting leave the source database fingerprints and every source image unchanged.

Successful runs remove only the two generated databases and their private synthetic fixture directory. Failures retain them for inspection. Logs and the manifest are private; the manifest contains synthetic bearer receipts and must not be posted publicly. A failed run does not authorize removal of a pre-existing database or broad workspace directory.

## Reproduce

Build the migration binary and test-only example:

```text
cargo build -p board-store --bin board-migrate --locked --jobs 1
cargo build -p board-public --example attachment_restore --locked --jobs 1
```

On the existing owned Linux test cluster:

```bash
sudo bash scripts/test-attachment-restore.sh
```

The wrapper loads private development role environments. The harness checks the `/tmp/board-postgres.*` marker against the running server's actual data directory before creating either database. CI builds workspace examples and runs this after the database-only restore.

On the owned Windows cluster provisioned under `.local/intake-postgres`:

```powershell
$db = Get-Content .local/intake-postgres/current-database.json -Raw | ConvertFrom-Json
. $db.env_path
python scripts/test-attachment-restore.py
```

The harness checks the private cluster marker, data directory, loopback port and PostgreSQL major version. Scratch files inherit the existing private Windows cluster ACLs. It accepts only expected role names and loopback endpoints, clears unrelated child credentials, and keeps passwords out of PostgreSQL command arguments.

Focused public-app clippy with all targets/features and warnings denied, formatting, Python compilation and shell syntax checks passed locally. The populated-attachment restore step passed on Linux for head `36672c8` in [PR CI](https://github.com/frankischilling/26chan/actions/runs/34732733196), including its missing-file and corrupt-file controls. This is same-cluster recovery evidence with the limits below.

## Production limits

This same-cluster synthetic exercise does not qualify fresh-host role bootstrap, encrypted offsite storage, backup-delete resistance, power-loss durability, point-in-time recovery, recovery objectives, or production filesystem/service identities. Reader handlers run inside the test process; native service and worker-isolation evidence remains separate.

Production restoration must pair database state with a consistent complete media store. Stop publishers and cleanup, or verify a coordinated snapshot procedure: a database dump alone does not freeze external files. Keep serving disabled until approval, integrity, grants, numbering and deletion checks pass. Reconcile deletions made after the selected backup before reopening reads; restoring old data without that history can resurrect removed content. Never run global reconciliation against a partial store copy.

Production backup ownership, retention and recovery requirements remain in [operations](operations.md#backup-and-recovery). This exercise enables no production service or backup policy.
