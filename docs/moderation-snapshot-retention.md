# Moderation snapshot retention

Independent moderation content snapshots expire after 30 days (720 hours), the
operator's chosen policy. Migration 0142 assigns each new snapshot a server-owned
deadline. A caller-supplied audit timestamp or deadline cannot extend that period.
The original source keeps a separate private deletion log; this 30-day limit is
an explicit privacy policy for the rewrite, not a claimed source default.

Expiry clears the saved name, tripcode, capcode, subject, comment, formatting
flags, filename, dice/fortune values and drawing annotations together. It also
clears the snapshot version. The audit record's identity, actor account, board,
target, action, timestamp and action-specific counts/masks remain. The original
expiry deadline remains as evidence that the content snapshot was retired.

This policy covers the independent snapshots in `content.moderation_audit`.
Report-time post captures have a different lifecycle: whole-post deletion removes
the target reports and their captures transactionally. This worker does not erase
live reports, report provenance, public content, shared sessions or media records.
Logs, backups and copies already exported from the database need separate operator
policies; clearing database values is not secure disk erasure.

## Automatic cleanup

`board-moderation-retention run` processes at most 16 transactions, each containing
at most 64 expired snapshots. Eligibility is inclusive at the transaction's start
time. Locked records are skipped and remain eligible for the next invocation.
A failed transaction rolls back its batch; previously committed batches stay
committed. Retrying after an uncertain result is safe because cleared snapshots
are no longer candidates.

The supplied service and timer run a bounded pass every minute. The deadline
makes a snapshot eligible for removal; it is not a promise of physical removal at
that exact instant. Scheduling delays, row locks, downtime and an initial backlog
can delay cleanup. Check failed service runs and repeated `batch_limit_reached`
results. Drain overdue snapshots before serving restored or upgraded audit history.

The worker returns only a count and a batch-limit flag. It has no direct table
read/write access and cannot request arbitrary IDs, an earlier cutoff or a longer
retention period. Its NOLOGIN function owner has only identifier/deadline reads
and writes to the snapshot payload columns. Public, staff, authentication and
media runtimes cannot call the expiry operation or assume its owner.

## Local use and deployment

1. On an existing database, apply `deploy/moderation-retention-role.sql` once as
   the bootstrap administrator, then apply migration 0142 with writers stopped.
   Fresh installations include these roles in `deploy/roles.sql`.
2. Configure the dedicated `board_moderation_retention` login through the existing
   secret-management process and grant it CONNECT on the application database.
   It needs no membership in another role. Do not use migration credentials for
   the worker; the command refuses them.
3. Build `cargo build -p board-store --bin board-moderation-retention --locked`.
   Set `MODERATION_RETENTION_DATABASE_URL` privately and run
   `target/debug/board-moderation-retention run` to qualify a local pass.
4. Schedule that command once per minute. On Linux, install
   `deploy/moderation-retention.service` and `deploy/moderation-retention.timer`,
   the binary at the service's fixed path, and a root-readable environment file
   at `/etc/paperboard/moderation-retention.env`. Enable the timer only after
   verifying the intended database and 30-day policy. An equivalent local
   scheduler can run the same bounded command on other supported systems.

Installing the migration does not erase any snapshot. Existing snapshots receive
an expiry based on their recorded audit timestamp, capped at migration time for
future-dated history. Their original fields remain unchanged until cleanup.
The deadline on new snapshots is always measured from the database clock at
insertion, independently of the supplied audit timestamp.

The schema checks the existing content catalog before migration and reseals its
reviewed changes afterward. Staff readiness and the worker verify the retention
functions, privileges and deadline trigger. Disabled guards, altered function
bodies and expanded worker permissions fail closed. Later schema changes require
the same review rather than an automatic runtime reseal.

## Restore and qualification

A backup made before expiry can contain a snapshot that has since expired.
Restore into a private environment, verify readiness, and run cleanup before
making that history available. Preserve the original stored deadline; do not
restart its 30-day clock when restoring. Keep the forward schema on application
rollback, since an older binary cannot restore erased snapshot values.

The database test checks exact cutoff equality, the next microsecond, deadline
ownership, batch limits, metadata preservation, rollback and denied runtime
access. `scripts/test-moderation-retention-migration.sh` checks fresh and populated
installation, unchanged original audit fields, the real dedicated-login command,
guard/ACL drift and an administrator restore containing overdue content. It also
checks that the command refuses migration credentials. Hosted final-head CI still
qualifies the complete application and service integration before merge.
