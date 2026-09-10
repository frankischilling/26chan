# Restricted media queue observer

Prompt section 11 requires queue saturation and processing-failure monitoring.
This slice adds actual database-backed queue observation and alert delivery. Host
storage/resource pressure, update checks, downstream monitoring authentication and
a deployed operator receiver remain separate requirements.

## Database contract

Migration 0010 creates schema `monitoring` and a security-barrier aggregate view
`monitoring.media_queue`, owned by the migration owner. A new `board_monitor`
role has only USAGE on this schema and SELECT on the view. It has no media,
content, secrets, staff-identity or deployment schema access, membership, ownership,
write, schema-creation or role-administration authority. Deployment bootstrap
creates it NOLOGIN; operator provisioning supplies an independent login password.
No existing runtime gains monitoring rights. Public/staff/media configuration
rejects inherited `MONITOR_DATABASE_URL`.

The view returns exactly one row of fixed numeric columns: capacity; receiving,
queued, processing counts; expired counts for those three states; oldest queued
age in whole seconds (zero when empty); and five failure counts in the preceding
15 minutes, by the existing fixed failure reasons. Use database statement time
for both expiry and the window. Count expired active jobs as occupying capacity
until reconciliation changes them. Exclude published jobs from active counts.
Index active-state aggregation and recent failed rows. Empty queues still return
the configured capacity and zero counts. A missing policy row is an error, not a
fabricated capacity. Do not expose IDs, filenames, bytes, hashes, lease tokens or
timestamps identifying individual jobs.

Recent failure counts are gauges. Existing terminal cleanup only removes rows
older than one day, so the 15-minute window remains observable across normal
cleanup. Do not invent lifetime counters or add privileged mutation triggers for
them. These gauges deliberately return to zero after the window. The view is
read-only, including when accessed through its owner-backed aggregate definition.

`board_store::monitoring::MonitorReader::connect(&str)` validates the exact login
and session identity, role flags, memberships, ownership, database/schema creation,
required SELECT and absence of base-table privileges before returning a reader.
Pool max 1, acquire timeout 1 second; each snapshot executes one aggregate SELECT
with a 2-second application deadline and statement timeout configured by the
operator. `snapshot()` returns typed `QueueSnapshot` fields (i64, checked
nonnegative; capacity 1..1024); `close()` releases the pool. Malformed, missing,
revoked or unavailable data is an error. No raw SQL errors or credentials are
logged by the service.

## Runtime and exporter contract

Add `board-monitor`. Require explicit APP_ENV development/production,
MONITOR_DATABASE_URL for board_monitor, and both existing METRICS_BIND_ADDR/TOKEN.
Development database URLs must be loopback. Production requires unambiguous
verify-full PostgreSQL TLS; reject unrelated credentials and non-Unicode config.
Bind the authenticated loopback exporter before database access. Validate the role
at startup. Do not expose another public application listener.

Run a single background sample immediately, then every five seconds with missed
ticks skipped; never overlap queries. Publish completed snapshots atomically. A
failed sample becomes unavailable immediately. A last success older than 30
seconds is also unavailable (monotonic elapsed time). Start unavailable. Keep the
last successful wall-clock timestamp for diagnostics, initialized to zero. A
recovery sample restores values. SQL never runs during an HTTP scrape.

Extend board-observe through `Metrics::register_media_queue(callback)` before
sharing. Its public `MediaQueueSample` holds `available: bool`,
`last_success_timestamp_seconds: u64`, `capacity: u64`, `active: [u64;3]`,
`expired: [u64;3]`, `oldest_queued_seconds: u64`, and `failures_recent: [u64;5]`.
State order is receiving/queued/processing; failure order is intake_failed,
abandoned, processing_failed, invalid_output, retry_exhausted. This callback only
copies memory. Reject duplicate registration. Fixed series:

- board_media_sample_success (0/1) and
  board_media_sample_last_success_timestamp_seconds (always emitted)
- board_media_queue_capacity
- board_media_jobs{state}, board_media_expired_jobs{state}
- board_media_oldest_queued_seconds
- board_media_failures_recent{reason}

Emit the latter five families only for an available snapshot. Do not retain stale
queue series or substitute zeros after failure. Existing HTTP metrics and pool
APIs retain their behavior. Add optional
`Endpoint::serve_with_health(metrics, application_future, readiness_callback)`:
authenticated /healthz returns 200, /readyz returns 200 or 503 without SQL. The
existing `serve` method does not gain those routes. Reuse all bearer validation,
no-store headers, 4 retained responses, 16 accepted connections, no keep-alive and
10-second connection deadline. Monitor shutdown handles Ctrl-C/SIGTERM and closes
its reader; exporter failure cancels sampling.

## Alerts and evidence

Add fixed project-defined rules: observer down/unavailable/stale for 30 seconds;
active capacity above 90% for one minute; processing_failed/invalid_output/
retry_exhausted in the recent window for 30 seconds; intake_failed/abandoned for
30 seconds; expired active jobs for one minute. Sampling failure alerts must work
when queue series vanish. Add board-monitor to candidate scrape configuration
with its own credential file and port 9194. Rule tests cover healthy, pending,
firing, recovery, missing data and retention boundaries; document thresholds.

Use an owned disposable PostgreSQL cluster/database with the actual observer
login, production view definitions and a real board-monitor process. Qualify
aggregate grants and denied base-table/writes using healthy owner controls;
seed synthetic queue saturation and processing failure through real queue
transitions; verify actual Prometheus/Alertmanager firing and recovery. Accelerate
only alert hold/scrape timing in transport qualification, not thresholds or SQL
retention semantics. Prove unavailable sampling causes 503 readiness and omitted
queue values, then restores after recovery. Cleanup only owned jobs, processes,
credentials and database/cluster. CI runs the full existing suite and this new
qualification before merge; local WSL remains unresponsive and is not restarted
without the pending approval. Keep production deployment claims unverified.

References: [PostgreSQL view privileges](https://www.postgresql.org/docs/16/sql-createview.html)
and [Prometheus instrumentation](https://prometheus.io/docs/practices/instrumentation/).
