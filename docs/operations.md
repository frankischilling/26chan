# Development and operations

## Service setup

The workspace contains the public application, separate staff application, operator migration/provisioning tools, and a development-only media intake command. PostgreSQL 16.15 is tested in a disposable WSL cluster. There is no Docker dependency in the development workflow. The systemd units are deployment candidates, not installed or verified production boundaries. See [media setup](media.md) and [staff setup](staff.md) for their credentials and operating commands.

The separate [Firecracker test profile](firecracker.md) creates real transient systemd services and disposable guests for local media qualification. It is an operator tool and has no authority through the public or staff application. Its root helper, artifact sources and service controls require review before any production adaptation.

For an owned staging host, create an OS account `board-public` without a login shell. Install only the public release binary under an operator-owned, read-only `/opt/paperboard`. Apply `deploy/roles.sql` once using a database bootstrap administrator; create the database owned by `board_migrator`, revoke PUBLIC database access, then grant CONNECT to the public and migration logins. Set distinct generated passwords through a secret manager, not committed SQL. Apply versioned migrations with the operator binary and seed boards deliberately. Synthetic fixtures belong only in test/development databases.

The bootstrap deliberately leaves `board_auth` and `board_staff` as NOLOGIN until staff provisioning. The production secret-management process must enable those two logins, supply distinct passwords, grant database CONNECT and set their statement/lock/idle-transaction limits before starting the candidate staff service. The development script performs those actions only on the verified disposable cluster; do not run it against production. Keep account administration in the separate operator process.

Migration 0008 also requires `board_media_read` to exist. The staging role template creates it as NOLOGIN with bounded database timeouts; it gains only the approved-asset view grant when migrations run. Keep it disabled until a media serving deployment is qualified. The development reader bootstrap creates a separate login only in the verified disposable cluster. `scripts/test-role-bootstrap.sh` applies the role template and every migration in an isolated Unix-socket-only cluster, then checks the reader's grants and disabled login.

Provide the public database URL through a root-owned `/etc/paperboard/public.env` readable only by systemd/operator, using `board_public` and `sslmode=verify-full` with a validated CA. Do not put migration, staff, media, release, cloud-management or backup credentials in that file. Startup rejects unrelated database credentials. The unit clears capabilities, prevents new privileges, restricts filesystem access and caps resources. Review its requirements against the target distribution before enabling it.

Terminate public HTTPS at an operator-controlled proxy and bind Axum to loopback or a private service address. Configure public, staff and media origins explicitly. If the optional [API listener](api.md) is enabled, configure its separate HTTPS origin and forward that hostname only to `API_BIND_ADDR`. Both listeners share the public process and its limits. Media requires a separate registrable domain from public, staff and API. Keep staff ingress separate and use its own host-only session cookies. A production firewall must allow each service only its required database/telemetry destinations and deny unrelated identity/deployment/metadata networks. These policies have not been deployed or tested. The proxy must strip incoming forwarded headers; the public application presently ignores all of them and rate-limits the socket peer, so proxy traffic shares one bucket.

## Health, limits and observability

The separate development [media HTTP reader](media-http.md) uses only the
approved-view login and generated output directory. Its candidate unit has
[actual disposable service evidence](verification-media-http.md), including a
distinct reader UID, read-only mounts, effective cgroup limits and denied
content/staff reads and media writes with healthy controls. It rejects
production startup. Follow the reader notes for explicit shared-publication
permissions, health semantics, resource limits and remaining deployment work.

In the public application, `/healthz` checks process availability and `/readyz` performs a content query, returning 503 on unavailable storage. Neither endpoint runs migrations. Public JSON logs record startup and generic database failures; handlers do not log post text, deletion passwords, URLs, IPs or raw SQL errors. The public candidate unit disables core dumps. Native dependencies and reverse-proxy logs need their own review.

The following table describes public limits. [Staff notes](staff.md) record the separate staff limits and readiness checks of both its authentication and moderation stores; [media notes](media.md) record intake and queue bounds.

| Limit | Implemented value | Evidence |
|---|---|---|
| URL-encoded public form body | 262,144 bytes, body collector limit; enough for 192,000 bytes of percent-encoded comment plus bounded fields | Maximum Unicode submission and overflow HTTP tests without Content-Length; the read-only API listener retains its 65,536-byte limit |
| Comment | Per-board Unicode scalar values, maximum 16,000; independent 64,000-byte UTF-8 ceiling | Domain/property, runtime-role SQL, HTTP and JavaScript-disabled browser tests |
| Requests | 32 admitted handlers and retained application response bodies/data across public and API listeners; 10-second handler deadline | Router/ownership tests cover held responses, emitted data, cancellation and recovery; external saturation/write-deadline exercise still required |
| Password hashing | 4 active Argon2 jobs; permits survive request cancellation until hashing finishes | Code; external memory enforcement unverified |
| Submissions | 30 per socket peer per 60 seconds; 10,000 tracked peers maximum | Forwarded-header rate test |
| Database pool | 12 connections, 3-second acquisition timeout | Code; production load testing pending |
| Database role defaults | 5-second statements, 2-second lock wait, 5-second idle transaction | Actual role configuration; concurrency tests |
| Candidate OS unit | 768 MiB memory, 2 CPU equivalents, 96 tasks, 1,024 file descriptors | Configuration only; no deployed enforcement claim |

Optional [private HTTP metrics](http-observability.md) expose fixed-label request
counts, handler durations/cancellations and existing database-pool occupancy through
a separate bearer-authenticated loopback listener. Candidate Prometheus rules cover
scrape availability, 5xx errors, rejected writes, staff 401/403 responses and pool
pressure. The owned notification qualification exercises synthetic firing and
recovery locally. The separate [queue observer](queue-observability.md) samples an
aggregate-only database view and alerts on saturation, failures, expiry and stale
observation. Its [verification record](verification-queue-observability.md) tracks
database and delivery evidence. The [authenticated monitoring profile](authenticated-monitoring.md)
adds verified HTTPS and independent request credentials to both notification links
and the native APIs, with broad native API authority documented explicitly.
The separate [resource observer](resource-observability.md) measures configured
filesystem capacity and local cgroup ceilings; its native qualification is tracked
in the verification record. The [maintenance recorder and observer](maintenance-observability.md)
cover configured update-command outcomes, overdue runs and unavailable journals.
Before launch, qualify actual update commands/schedules and storage/resource monitoring
on the deployed host, and configure and
verify an operator notification destination. Do not treat journal output, a
successful scrape, or local webhook delivery as equivalent production coverage.

Comment limits count Rust `chars()` and PostgreSQL `char_length` in a UTF8 database. Combining marks and received CR/LF characters count separately; no normalization is performed. A maximum comment can now occupy 64,000 bytes before HTML escaping. Thread and catalog row caps do not establish a safe aggregate response-memory budget. Mixed load, large responses and the candidate OS memory ceiling still need external qualification.

Admission remains occupied after a handler returns while its response body or emitted data is retained. The final body layer runs after API error/HEAD/OPTIONS transformations; data clones and slices share the permit owner. Dropping an unconsumed response or releasing its completed data frees capacity. Empty responses release immediately. This is a count limit, not a byte budget or acknowledgement that the client received data. Small overload responses, kernel buffers and copies made by downstream consumers are outside it. Configure and exercise proxy connection, header and response-write timeouts: a slow client can still occupy a slot, and the ten-second handler timeout does not cover the socket write. See [admission verification](verification-response-admission.md).

## Backup and recovery

`scripts/restore-exercise.sh` uses PostgreSQL's custom dump/restore format in an owned disposable cluster. It creates a distinct restore database, compares post and asset fingerprints plus fifteen table counts, and exercises approved-reader, aggregate-observer and public/media/authentication/moderation grants and denials. It drops that generated restore database on success and leaves the backup under ignored `.local/backups/`; a failed run may need operator removal of its restore database. Run it with writes stopped; changes during comparisons cause a failure. It restores database metadata, not media object files, and does not establish point-in-time recovery or production backup durability. Current outcomes are in [the approval verification record](verification-media-approval.md).

Production requires an independent backup identity and storage account. Application and staff credentials must be unable to delete backups, change retention or access encryption recovery keys. Set and document retention from the operator's privacy/recovery requirements; no production retention period has been chosen. Keep encrypted, immutable copies outside the serving account. Schedule periodic restoration on a separate network, verify data and grants, then record recovery time and recovery point. Those measurements have not been taken here.

To stop the disposable cluster without deleting data:

```bash
source .local/database.env
sudo -u postgres /usr/lib/postgresql/16/bin/pg_ctl -D "$BOARD_TEST_CLUSTER" -m fast -w stop
```

The recorded `/tmp/board-postgres.*` data directory can be restarted while it remains available. `/tmp` is deliberately unsuitable for durable development or production data. Before cleanup, resolve and verify the exact recorded path; do not recursively remove a computed path without checking it. Keep the backup or discard it through a deliberate operator action.

## Releases, rotation and incident response

Migration 0009 adds [thread rollover and optional archives](thread-archives.md).
It preserves historical data and leaves archives disabled. Stop public and staff
serving before migration, then start matching binaries. Old binaries do not
understand archive visibility; binary-only rollback after archiving is unsafe.
Retention policy affects public visibility and does not physically erase retained
base tables or backups. The actual upgrade exercise is recorded in
[archive verification](verification-thread-archives.md).

Build release artifacts from a reviewed commit with the lockfile. Run migrations using the operator identity before starting compatible application code. Migrations are forward-only; rollback of destructive schema changes requires a reviewed compensating migration or restoration. Do not assume replacing a binary reverses a migration. No release automation or production deploy has run.

For migration 0007, stop public and staff serving, take a backup, apply the operator migration, and start binaries that use `max_comment_chars` and the expanded parser bound. The migration requires UTF8 encoding and preserves existing numeric board settings and post text. It renames the board column and replaces the global comment constraint. Old public binaries expect the old column, and old renderers truncate longer comments; do not roll back only the binaries. Review proxy form-body limits alongside the 256 KiB application limit. `scripts/test-comment-migration.sh` exercises the historical upgrade and encoding guard in separate disposable databases. See [comment verification](verification-comment-limits.md).

Rotate database credentials through the operator channel, restart affected pools and revoke the old credential. Staff authenticator revocation and recovery use the separate operator workflow described in [staff operations](staff.md); normal staff runtime credentials cannot change account roles. Rotate release and backup credentials outside web services. Re-run permission and recovery tests after grant changes.

If the public process is compromised, isolate it, revoke its database credential, preserve restricted forensic evidence and rebuild from a trusted artifact. Review content mutations and report spam; public compromise can expose retained deleted text and password hashes. Do not assume staff identities were exposed solely because they share the server: verify actual grants and evidence. If future media workers are compromised, stop promotion/intake, destroy guest workspaces, preserve bounded forensic records and patch the guest/host stack before resuming. The planned worker must not have credentials to revoke from application or staff databases in the first place.
