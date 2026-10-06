# Deletion quota verification

Production deletion requires a stable `POSTER_ID_KEY` and a verified transport
peer. The database admits three successful requests per hour and eleven per day.
There is no development bypass. Migration 0085 requires the role bootstrap in
`deploy/public-deletion-role.sql` on existing installations before the migration.
Fresh disposable databases receive the role through `deploy/roles.sql`.

## Counter and authority tests

`crates/store/tests/public_deletion_quota.rs` checks inclusive boundaries,
concurrent admission, rollback, capacity, expiry and private-role permissions.
The capacity fixture is transactional and must run in a serial disposable
qualification lane. `apps/public/tests/deletion_quota.rs` checks real HTTP
requests and stored outcomes, including cookie changes, forged headers, legacy
partial success, file deletion and flood-before-target error precedence.

These tests do not use the browser isolation helper. Their quota history remains
intact throughout each scenario. Existing authority and delayed-lock tests still
exercise the same mutation paths with explicit synthetic peer identities.

## Independent browser workflows

Many UI tests share one loopback peer. Their setup and teardown must not consume
the allowance of an unrelated UI case. The gated `deletion-quota-fixture` example
provides test-only isolation; it is not part of the public server.

The real-server Playwright configuration prepares a fresh per-run key in memory.
Global setup creates a private manifest and an expiring ownership lease after
checking serial execution, development mode, the local migration identity and
known synthetic database fixtures. Config listing and failed server startup do
not create a manifest or lease. The public server receives its identity key but
not migration credentials or the manifest path.

Each explicit `withDeletionQuota` group resets only the actor derived from that
run's key and fixed loopback peer. Groups are serialized and contain no more than
three successful deletion requests. Requests, browser interactions, status checks
and persistence assertions still run normally. There are no intercepted requests,
mutating retries, changed production thresholds or whole-table resets. Separate
runs have distinct actors. Teardown removes only the owned actor and lease; a
failed retirement preserves its manifest for exact cleanup retry.

The standalone Rust upload and privileged media fixtures generate their own
keys and isolate only their own actors between independent workflows. They still
require exactly one recorded successful deletion per workflow. None of this is
proof of quota enforcement; the dedicated counter and HTTP tests provide that.

Run `scripts/verify.sh` with disposable database variables. It builds the gated
helper and establishes development mode after rejecting a conflicting mode.
Direct real-server Playwright commands also require `APP_ENV=development` and
the prebuilt helper. Visual-only configurations need neither a database lease nor
the helper. The Node orchestration unit suite currently uses a POSIX executable
stub; Windows visual qualification is separate.
