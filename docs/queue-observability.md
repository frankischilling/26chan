# Media queue observation

`board-monitor` reads one aggregate view with a dedicated PostgreSQL login. It
cannot read individual jobs, filenames, lease tokens, approved assets, posts or
staff credentials. Migration `0010_media_monitoring.sql` adds the owner-backed,
security-barrier view and two partial indexes. Bootstrap `board_monitor` as
NOLOGIN before migrating an existing installation; supply an independent password
and database CONNECT when provisioning the service. Do not give it role membership,
database TEMP/CREATE, schema CREATE, base-table access or object ownership.

The bootstrap sets a two-second statement timeout, one-second lock timeout and
two-second idle transaction timeout. Startup checks the actual session/login,
role flags, privileges, view owner and statement timeout. One pooled connection
samples immediately and every five seconds; each sample has a two-second
application deadline. Failed samples immediately remove readiness and queue data.
Successful samples older than 30 monotonic seconds are also unavailable. Scrapes
and health checks only copy cached state. Restart to revalidate privilege changes;
sample failures recover without restarting when the same view grant is restored.

## Configuration

Use a service environment containing only its own settings:

```text
APP_ENV=production
MONITOR_DATABASE_URL=postgres://board_monitor:INDEPENDENT_SECRET@db.example.invalid/imageboard?sslmode=verify-full&sslrootcert=/etc/paperboard/database-ca.pem
METRICS_BIND_ADDR=127.0.0.1:9194
METRICS_TOKEN=INDEPENDENT_64_LOWERCASE_HEX_TOKEN
```

These placeholders are not runnable credentials. Use URL encoding for passwords.
Production requires explicit `sslmode=verify-full`; the database host must be TCP,
not an encoded socket path. Development requires explicit `APP_ENV=development`
and a loopback database host. Both modes require an explicit nonempty URL password,
reject duplicate/unknown connection parameters and reject other application
credentials or inherited `PG*` connection overrides. Non-Unicode values fail
closed. `METRICS_BIND_ADDR` must be a nonzero loopback socket and `METRICS_TOKEN`
exactly 64 lowercase hex characters. Binding happens before database connection.

For the owned development cluster, run `scripts/dev-monitor-db.sh` before migration
and source its ignored `.local/monitor.env` only into the observer or test runner.
Public, staff and media processes reject that credential. The test runner filters
it out of their child environments. The [candidate unit](../deploy/monitor.service)
uses a separate OS identity and resource limits; these are configuration, not
evidence of enforcement on an unqualified deployment host.

All three private routes require the same single Bearer header: `/metrics`,
`/healthz` (200 while serving), `/readyz` (200 with a fresh valid sample, otherwise
503). They have no CORS and use no-store/nosniff. The exporter accepts at most 16
connections, expires each after ten seconds and retains at most four exposition
responses/data owners. Health responses are empty and remain available while
exposition slots are occupied. Ctrl-C/SIGTERM cancels the sample and closes the
pool; exporter termination also cancels sampling.

## Values and alerts

| Metric family | Meaning |
| --- | --- |
| `board_media_sample_success` | Fresh valid cached sample, 0 or 1 |
| `board_media_sample_last_success_timestamp_seconds` | Last successful wall-clock timestamp, initially 0 |
| `board_media_queue_capacity` | Configured queue capacity |
| `board_media_jobs{state}` | Receiving, queued and processing job counts |
| `board_media_expired_jobs{state}` | Active jobs whose expiry has passed |
| `board_media_oldest_queued_seconds` | Whole seconds since the oldest currently queued job was created, 0 if empty |
| `board_media_failures_recent{reason}` | Counts for the five fixed reasons in the preceding 15 minutes |

The first two families always appear. The remaining five disappear on unavailable
sampling; missing data must never be interpreted as a healthy empty queue.
Expired jobs still occupy capacity until reconciliation changes their state.
Recent failure values are gauges, not lifetime counters: they become zero as rows
leave the inclusive 15-minute database statement-time window. Normal cleanup
removes terminal jobs only after one day, preserving this observation window.

The [candidate rules](../deploy/monitoring/alerts.yml) alert on unavailable/down or
stale observation for 30 seconds, capacity above 90% for one minute, recent
processing or intake failures for 30 seconds, and expired jobs for one minute.
Workload alerts require a fresh available sample and healthy scrape. Configure
the separate `board-monitor` scrape token in the [monitoring examples](../deploy/monitoring/README.md).

## Qualification

Build the observer and owned fixture, download the pinned monitoring tools, then
run on an owned disposable Linux host with PostgreSQL 16:

```sh
cargo build -p board-monitor --bin board-monitor --example queue_fixture --locked
python3 scripts/monitoring/download.py
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  bash scripts/test-queue-monitoring.sh "$PWD/target/debug/board-monitor" \
  "$PWD/target/debug/examples/queue_fixture" "$PWD/.local/monitoring/bin"
```

The helper creates its own cluster and generated role passwords. Real reservations
fill capacity four; a fifth is rejected. Releasing reservations resolves pressure.
A real queued/claimed job records a processing failure. The owner ages only test
failures by 16 minutes to exercise resolution without changing the SQL window.
Prometheus and Alertmanager must deliver firing and resolved notifications to the
owned loopback receiver. Revoking/regranting view SELECT must remove/restore
readiness and queue series. Only notification hold/scrape timing is accelerated.
Owned children, fixture data, grants and credentials are cleaned on exit.

Repeat the helper with `QUEUE_QUALIFICATION_INTERRUPT=1` in its explicit `sudo env`
environment to verify OS SIGTERM cleanup after a real healthy scrape. The watcher
checks the three owned child identities and temporary token directory disappear;
the outer helper verifies zero jobs, capacity 64 and restored observer SELECT
before removing its cluster. Temporary files stay inside that owned cluster tree.

Read [verification evidence](verification-queue-observability.md) for actual
outcomes. This does not qualify a production receiver, downstream monitoring-hop
authentication, host/database storage/resource monitoring, update checks or
production media deployment.
