# Authenticated media intake implementation plan

> For agentic workers: use superpowers:subagent-driven-development task by task.

**Goal:** Connect authenticated HTTP intake to the existing real quarantine,
isolated processing, approval and reader pipeline under a narrow intake identity.

**Architecture:** A separate development-only Axum service authenticates callers
and reservation capabilities. Narrow SECURITY DEFINER functions mediate queue
admission and receiving transitions; processing/publication retain their current
identities. Existing public upload and production enablement guards remain closed.

**Tech stack:** Rust 1.94.0, locked Axum/SQLx/Tokio, PostgreSQL 16, Python 3.12,
existing Firecracker/jailer 1.16.1 profile, tokio-util 0.7.19 StreamReader.

**Spec:** `docs/superpowers/specs/2026-09-10-media-intake-design.md`.

## Global constraints

- Work in the requested checkout on `feature/media-intake`, based on main
  `c935adb6cfcbfe7d03c49edf48a3232927b0dd35`. Preserve unrelated files and 4chan-old.
- Use the installed humanizer, Git author and Git workflow skills. Use the
  PowerShell Git workflow helper for Git/GitHub changes; preserve configured identity.
- No public upload activation, production deployment, decoder in intake, raw
  fallback, new processing authority, changed screenshot baselines or invented parity.
- No real user/credential fixtures, secret-bearing logs or real-world exploit tests.
- Bounds: input 8,388,608 bytes plus one lookahead; JSON 1,024 bytes; eight
  requests, four upload bodies, receive 15 seconds, handler 20 seconds.
- Keep normal first-party Rust safe and linted. Do not disable tests/diagnostics.
- WSL is currently unresponsive. Native database/VM/service evidence must run in
  hosted CI until an owned local environment is available; label local skips.
- Windows builds can use private `.local/rustup-staff` and
  `.local/staff-shutdown-target`, portable Perl, and `--jobs 1`; parallel checks
  previously failed metadata resolution. Do not change global toolchain settings.

## Task 1: Capability-scoped database intake

Files: `migrations/0011_media_intake.sql`, `crates/store/src/media_intake.rs`,
`crates/store/src/lib.rs`, `crates/store/tests/media_intake.rs`,
`deploy/roles.sql`, `scripts/dev-intake-db.sh`,
`scripts/test-role-bootstrap.sh`, `scripts/test-media-intake-migration.sh`.

Consumes the existing queue schema, expiry/cleanup and approved assets. Produces
all exact IntakeStore/IntakeReservation/IntakeStatus interfaces and SQL function
signatures in the spec. It owns no HTTP files. The controller wires CI/restore in
Task 3, including the new login before migrations and INTAKE_DATABASE_URL tests.

- [ ] Add database-tests coverage using actual intake/migrator/media logins. Begin
  with a reservation, wrong-capability status/claim denial and processing-claim
  denial, then implement the full matrix from the spec. Example test consumer:

```rust
let store = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await.unwrap();
let reservation = store.reserve("synthetic.png").await.unwrap();
assert!(matches!(store.begin_upload(&reservation.id, &"0".repeat(64)).await, Err(StoreError::NotFound)));
store.begin_upload(&reservation.id, &reservation.capability).await.unwrap();
assert!(matches!(store.begin_upload(&reservation.id, &reservation.capability).await, Err(StoreError::Conflict(_))));
store.finish_upload(&reservation.id, &reservation.capability, 32).await.unwrap();
assert_eq!(store.status(&reservation.id, &reservation.capability).await.unwrap().state, "queued");
```

- [ ] Run the initial test/check and preserve the actual failing result. Native
  tests require the new provisioned login; compile-only or unavailable database
  output is not a passing denial test.
- [ ] Implement versioned schema/roles, fixed-path definer functions, atomic
  capacity/claim transitions, scoped errors and startup role/grant validation.
  Use the existing bootstrap script's owned-cluster checks and private env files.
- [ ] Add concurrency tests for two begin calls (one winner), expiration under a
  lock wait, no different receiving job mutation, real capacity, forbidden
  tables/DDL/roles/lease claim/approval, deleted handles and approved-only output.
  Verify the NOLOGIN owner has only the required underlying authority.
- [ ] Implement migration test from 0010 in a disposable database: existing jobs
  have no fabricated handles; existing queue/approval behavior and data remain;
  new login/function grants work; transactional migration rollback is safe.
- [ ] Run formatting, compile/tests possible locally and precise native commands
  when available. Commit through the helper and report results/limitations for
  source review; native proof remains mandatory before final branch merge.

## Task 2: Authenticated bounded intake server

Files: `apps/media-intake/Cargo.toml`, `apps/media-intake/src/{lib,main,config,http}.rs`,
`apps/media-intake/tests/{config,http,database}.rs`, root workspace manifest/lock,
`crates/observe/src/lib.rs` and its focused listener tests. Consume Task 1's exact
store API and existing board-media Quarantine/board-http response ownership.
Produce `board-media-intake` and an ordinary public `router(state)` testable with
the actual store; no test-only production endpoints or injected permit bypasses.

- [ ] Add failing portable config/auth/header/limit tests and database-backed HTTP
  tests. Start with unauthorized POST denying before a lazy unavailable store is
  touched, then test actual reservation and streaming against the real login.
- [ ] Implement strict development-only config and secret-free errors; reject
  nonloopback bind/database, ambiguous URL options and unrelated credentials.
- [ ] Implement exact service-auth/capability checks and the five HTTP routes in
  the spec, using a 1-KiB Json limit and the body's data stream for raw PUT:

```rust
let stream = body.into_data_stream().map_err(std::io::Error::other);
let reader = tokio_util::io::StreamReader::new(stream);
let result = tokio::time::timeout(Duration::from_secs(15), quarantine.receive(id, reader)).await;
// Success: finish_upload; uncertain finish retains completed private input.
// Receive error/deadline: PartialFile removes partial bytes; attempt abort_upload.
```

- [ ] Add exclusive claim before receive, admission/body limits and response-held
  permits, coherent error codes, no-store security headers and intake metrics.
  Add real SIGTERM/SIGINT shutdown handling and close the database pool/listeners.
- [ ] Cover absent-length/chunked overflow, empty input, broken body, slow input,
  concurrent duplicate PUT, missing/wrong capability, unavailable grants/storage,
  completed-file retention after uncertain queue finish, token/filename omission
  from responses/logs and startup production rejection. Tests assert effects,
  not source/config strings; bounded tests do not create exploit payloads.
- [ ] Run pinned local formatting, portable tests and compile checks. Preserve
  failures and native limitations; commit and receive scoped source review.

## Task 3: Owned full-path qualification and operations

Files: `deploy/media-intake.service`, `deploy/media-intake.env.example`,
`tests/media/test_intake_service.py`, optional focused fixture module under
`tests/media`, `scripts/test-media-intake.sh`, `.github/workflows/ci.yml`,
`scripts/verify.sh`, `scripts/restore-exercise.sh`, existing runtime config
credential-denial lists/tests, docs architecture/media/operations/readiness/
compatibility/dependencies and `docs/verification-media-intake.md`.

Consume the real intake binary, its schema/login and existing
`tests/media/dispatch_service_fixture.py` service harness. Produce a repeatable
root-owned Linux test command using generated disposable state and identities.

- [ ] Implement a narrow standard-library HTTP fixture client: store service and
  object credentials only in memory/private files, bound all reads/timeouts,
  never follow redirects or print secret requests. Drive reserve, PUT and status.
- [ ] Start the actual candidate intake unit under a distinct UID, not the test
  operator. Keep only INTAKE_DATABASE_URL in its DB environment, owned quarantine
  write authority and a separately authenticated metrics endpoint. Verify real
  effective filesystem/capability/resource restrictions and healthy private-file
  controls. Use current root operator dispatch authority explicitly; do not
  claim a qualified production coordinator.
- [ ] Send synthetic still PNG through HTTP; use the existing authenticated
  dispatcher and actual Firecracker; require approved status and the same
  generated output through the separate HTTP reader. The client never supplies
  a path, lease, dimensions, approval or worker command. Rejected/partial/oversized
  input must not appear in approval or HTTP output.
- [ ] Exercise wrong/missing service and object credentials, forbidden-origin
  requests, concurrent upload ownership, bounded timeout/disconnect, queue/store
  failures, terminal cleanup and a subsequent successful job. Verify normal and
  OS SIGTERM cleanup of every owned service/process/listener/private fixture.
- [ ] Wire dev-intake-db before migration in CI, source only the needed test
  credential into database suites, add actual historical migration and full-path
  qualification, and extend restore checks to handle counts and granted/denied
  intake operations. Add new credential rejection to all unrelated runtimes and
  their tests. Preserve existing full/native/visual/advisory checks.
- [ ] Record operation commands, identity/storage scopes, two-phase retry rules,
  actual evidence and unqualified production/post-attachment requirements.
  Update dependencies for tokio-util and any existing newly direct library pins.
- [ ] Run full required checks, inspect native outcomes, fix failures without
  weakening assertions, and request whole-branch review. Push/update draft PR
  linked to #46 and merge only after final-head CI passes and review is resolved.

## Review and execution record

Use this plan's `.superpowers/sdd` ledger for per-task briefs, source-review
outcomes, exact commits, native pending checks and any rulings. Do not label a
task's native behavior verified from a portable compile or mock. Database and
service work can continue after a clean conditional source review while their
combined native execution awaits Task 3 wiring; final completion requires that
execution. This avoids inventing local WSL results or leaving independent work
idle. Do not dispatch concurrent implementers into shared files.
