# Robot9000 staff cleanup verification

Local qualification on 2026-10-08 used the integrated paired-processing source
at `cb836c7` with migration 0117 and the cleanup changes. No hosted result is
claimed for this change.

## Passed locally

- The hash-pinned PHP extractor generated and then reproduced all 96 permission
  cases. Rust compares every case with the cleanup policy.
- All 46 staff library tests passed, including source Options/badge policy and
  the new strict form parser.
- All 11 cleanup database tests passed on a fresh owned PostgreSQL 16 cluster.
  They cover role/scope/global-developer gates; disabled and private retained
  history; 1,000-row batches and zero-row audits; unchanged recent hashes and
  mutes; exact fields, CSRF, origin, recent authentication and 4KiB body limits;
  audit-failure rollback; independent-connection board locking; lock timeout;
  authentication expiry while waiting; actual private-role denials; and six
  required/forbidden privilege-drift checks.
- UTC calendar expression tests cover leap days, timezones and strict
  before/equal/after boundaries. Live-function tests bracket its server clock
  separately; they do not inject a clock into the production function.
- All three existing Robot9000 store tests passed, including progressive mutes,
  duplicate concurrency, retained deletion history, capacity failure and
  rejected attachment rollback.
- Strict staff Clippy passed across all targets and features. Formatting,
  edited-document local links, JavaScript syntax and the upgrade script's
  shell syntax passed.
- Fresh role bootstrap applied all migrations as the migration owner and passed
  its grant/privacy assertions. The populated 0116-to-0117 upgrade preserved
  history and existing functions; cleanup and audit rolled back together.
  Local wrappers used an owned unprivileged PostgreSQL process on loopback TCP
  because this executor cannot create Unix sockets. The committed bootstrap and
  upgrade scripts retain the existing privileged CI workflow.
- Staff binaries/examples, public binaries, intake binaries and the owned
  deletion-quota browser fixture built successfully.

Earlier broader regressions passed 3 staff database tests, 8 permission tests,
5 HTTP tests and 31 posting tests. Two existing Unix-peer tests could not create
sockets in this executor and failed with `Operation not permitted`. One HTTP
rerun explicitly skipped its blocked test; neither test nor its assertions was
changed. This is not a full staff regression pass.

## Browser qualification is still pending

The new real browser workflow enters from the report queue, exercises rank and
scope gates, and submits script-free batches of 1,000, 7 and 0. It checks the
stored counts, audit totals and unchanged mute state. Invalid CSRF and expired
authentication are exercised by the HTTP integration tests, not this browser
case.

An actual local attempt used existing Chromium 154 and Node 24.19.0. Service
startup and the owned seeded quota fixture succeeded, but Chromium aborted at
`process_singleton_posix.cc` because `socket()` returned `EPERM`. It also reported
a read-only crash-settings path. The browser assertions never ran. The ordinary
CI configuration and assertions are unchanged; its pinned environment must
qualify this workflow and the complete suite at the published head.

## Failed setup attempts retained

The first PHP invocation lacked its private `libargon2` search path. The path
was supplied before generation and reproduction passed. A populated-upgrade
wrapper initially tried to assume the staff role from the migrator, which was
correctly denied; the fixture administrator now performs that isolated role
switch. The first store regression invocation omitted media connection variables;
all three tests passed after supplying the owned fixture connections.

Initial browser setup attempts lacked a password in the synthetic intake URL,
the existing deletion-quota helper binary, or the expected demo seed. Those
fixture prerequisites were supplied without weakening runtime validation or
ownership checks. The subsequent browser launch failed on the socket restriction
above. A first Clippy run found a constant after the test module; moving the
constant resolved the lint without an allowance.

These results qualify the local cleanup implementation, not completion of #222,
#191, production deployment, or the full source rewrite.
