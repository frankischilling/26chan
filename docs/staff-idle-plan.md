# Staff inactivity timeout

The report queue currently accepts an unattended session until its eight-hour absolute expiry. Add an independently enforced inactivity deadline without changing public behavior or weakening the existing ten-minute reauthentication requirement. This is project-defined staff security behavior, not a claim about original moderation internals.

## Global constraints

- `STAFF_IDLE_TIMEOUT_SECONDS` defaults to 900; accept integer seconds from 60 through 3600, rejecting empty, malformed, zero and out-of-range values at startup. This operational range is a project policy.
- Store activity in PostgreSQL. Every successful session authentication checks current account/credential/role state and both deadlines before atomically advancing activity using database time. An expired session cannot be revived by a request or a concurrent request waiting for its row lock.
- Activity never changes `authenticated_at`, `expires_at`, the session token, or cookie expiry. Moderation still requires WebAuthn authentication within ten minutes; absolute lifetime remains eight hours.
- Only protected requests that perform session authentication count as activity. Static files, login pages and health checks do not. Authentication may succeed before a later CSRF, object or reauthentication check denies a request; that request still counts as activity.
- Add a forward-only migration with least-privilege column grants. Pre-migration sessions have no activity evidence after authentication, so backfill from `authenticated_at`, never the migration time. Coordinate staff restart with migration; an old binary would not enforce inactivity.
- Keep public/media/staff database boundaries, token hashing, exact-origin/CSRF protection, fail-closed behavior and private/no-store responses intact. No production deployment, merge, dependency changes or media enablement.

## Implementation and verification

1. Add failing PostgreSQL tests for idle denial, live activity extension, fixed absolute/recent-auth timestamps, independent sessions, runtime column privileges and concurrent checks. Implement typed policy, migration, atomic authentication and idle cleanup on login. Readiness must fail if the new schema is absent.
2. Extend the existing browser fixture and real WebAuthn flow to expire activity with cookies still present, deny queue access and moderation without audit changes, then require a new WebAuthn login. Assert activity does not refresh recent authentication. Document configuration and migration/rollback behavior.
3. Run formatting, strict clippy, locked workspace build/tests, both browser suites, advisory checks and the disposable restore exercise. Review the branch, fix material findings, commit and publish a draft PR linked to its issue. Record actual results and unchanged launch blockers.

Reference: [OWASP session expiration guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-expiration) separates server-enforced idle and absolute deadlines. [PostgreSQL 16 Read Committed behavior](https://www.postgresql.org/docs/16/transaction-iso.html#XACT-READ-COMMITTED) rechecks an update predicate after a concurrent row change; tests must exercise the actual database behavior. Consulted 2026-09-08.
