# Development and operations

## Service setup

The runnable services are the public application and an operator-only migration CLI. PostgreSQL 16.15 was tested in a disposable WSL cluster. There is no Docker dependency in the development workflow. `deploy/public.service` is a candidate systemd unit, not an installed or verified production deployment.

For an owned staging host, create an OS account `board-public` without a login shell. Install only the public release binary under an operator-owned, read-only `/opt/paperboard`. Apply `deploy/roles.sql` once using a database bootstrap administrator; create the database owned by `board_migrator`, revoke PUBLIC database access, then grant CONNECT to the public and migration logins. Set distinct generated passwords through a secret manager, not committed SQL. Apply versioned migrations with the operator binary and seed boards deliberately. Synthetic fixtures belong only in test/development databases.

Provide the public database URL through a root-owned `/etc/paperboard/public.env` readable only by systemd/operator, using `board_public` and `sslmode=verify-full` with a validated CA. Do not put migration, staff, release, cloud-management or backup credentials in that file. Startup rejects inherited migration/staff database variables. The unit clears capabilities, prevents new privileges, restricts filesystem access and caps resources. Review its requirements against the target distribution before enabling it.

Terminate public HTTPS at an operator-controlled proxy and bind Axum to loopback or a private service address. Configure the three origins explicitly. Media requires a separate registrable domain. Keep staff ingress separate; no staff application exists yet. A production firewall must allow the public service only its required database/telemetry destinations and deny identity/deployment/metadata networks. These policies have not been supplied or tested. The proxy must strip incoming forwarded headers; the application presently ignores all of them and rate-limits the socket peer, so proxy traffic shares one bucket.

## Health, limits and observability

`/healthz` checks process availability. `/readyz` performs a content query and returns 503 on unavailable storage. Neither endpoint runs migrations. JSON logs currently record startup and generic database failures; no post text, deletion passwords, URLs, IPs or raw SQL errors are logged by handlers. Production crash dumps are disabled in the candidate unit. Native dependencies and reverse-proxy logs need their own review.

| Limit | Implemented value | Evidence |
|---|---|---|
| URL-encoded request body | 65,536 bytes, body collector limit | HTTP test with no Content-Length |
| Comment | Per-board UTF-8 bytes, maximum 16,000 | Domain/property tests and SQL constraint |
| Requests | 32 active, 10-second handler deadline | Code; external saturation/deadline exercise still required |
| Password hashing | 4 active Argon2 jobs; permits survive request cancellation until hashing finishes | Code; external memory enforcement unverified |
| Submissions | 30 per socket peer per 60 seconds; 10,000 tracked peers maximum | Forwarded-header rate test |
| Database pool | 12 connections, 3-second acquisition timeout | Code; production load testing pending |
| Database role defaults | 5-second statements, 2-second lock wait, 5-second idle transaction | Actual role configuration; concurrency tests |
| Candidate OS unit | 768 MiB memory, 2 CPU equivalents, 96 tasks, 1,024 file descriptors | Configuration only; no deployed enforcement claim |

Prometheus metrics and alert delivery are not implemented. Before launch, add and exercise alerts for request/authorization error rates, rejected writes, pool pressure, database storage, update-check failures, and future media queue depth/failures/output limits. Do not treat journal output as equivalent coverage.

## Backup and recovery

`scripts/restore-exercise.sh` uses PostgreSQL's custom dump/restore format in an owned disposable cluster. It creates a distinct restore database, compares an ordered fingerprint of all post rows and counts for eight other tables, proves public reads and protected staff denial after restoration, then drops only that generated restore database. It leaves the backup under ignored `.local/backups/`. Run it with writes stopped; changes during comparisons cause a failure. The exercised restore does not establish point-in-time recovery or production backup durability.

Production requires an independent backup identity and storage account. Application and staff credentials must be unable to delete backups, change retention or access encryption recovery keys. Set and document retention from the operator's privacy/recovery requirements; no production retention period has been chosen. Keep encrypted, immutable copies outside the serving account. Schedule periodic restoration on a separate network, verify data and grants, then record recovery time and recovery point. Those measurements have not been taken here.

To stop the disposable cluster without deleting data:

```bash
source .local/database.env
sudo -u postgres /usr/lib/postgresql/16/bin/pg_ctl -D "$BOARD_TEST_CLUSTER" -m fast -w stop
```

The recorded `/tmp/board-postgres.*` data directory can be restarted while it remains available. `/tmp` is deliberately unsuitable for durable development or production data. Before cleanup, resolve and verify the exact recorded path; do not recursively remove a computed path without checking it. Keep the backup or discard it through a deliberate operator action.

## Releases, rotation and incident response

Build release artifacts from a reviewed commit with the lockfile. Run migrations using the operator identity before starting compatible application code. Migrations are forward-only; rollback of destructive schema changes requires a reviewed compensating migration or restoration. Do not assume replacing a binary reverses a migration. No release automation or production deploy has run.

Rotate public database credentials by provisioning the replacement secret through the operator channel, restarting affected pools and revoking the old credential. Staff credential/session rotation and WebAuthn revocation need a separate implementation. Rotate release and backup credentials outside web services. Re-run permission and recovery tests after grant changes.

If the public process is compromised, isolate it, revoke its database credential, preserve restricted forensic evidence and rebuild from a trusted artifact. Review content mutations and report spam; public compromise can expose retained deleted text and password hashes. Do not assume staff identities were exposed solely because they share the server: verify actual grants and evidence. If future media workers are compromised, stop promotion/intake, destroy guest workspaces, preserve bounded forensic records and patch the guest/host stack before resuming. The planned worker must not have credentials to revoke from application or staff databases in the first place.
