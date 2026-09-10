# Thread archive verification

This change implements the pinned archive contract in compatibility I-006,
archive board/OP fields, and board rollover. Local checks ran September 10, 2026
with Rust 1.94.0, PostgreSQL 16.15 in the owned WSL cluster, Node 25.2.1 and pinned
Playwright 1.62.0/Chromium 151. No registry dependency or lockfile changed.

## Behavior and regression evidence

- The first real-public-role test failed because a full board rejected its
  second OP. Atomic rollover then passed for disabled and enabled archives.
- Store tests cover expiry across thread/post/quote/report/deletion lookups,
  archive capacity, disabled policy, fixed expiry after policy increases,
  sage/bump order, mixed and all-pinned boards, and concurrent new OPs.
- Actual PostgreSQL lock queues enforce both reply-versus-rollover orders. A
  reply that commits first persists and changes the rollover victim; a rollover
  that commits first makes the subsequent reply fail without adding a post or
  incrementing its lifetime count. A table-lock barrier commits archive policy
  between the two read statements and verifies one coherent response snapshot.
- A regression reproduced expired newer entries wrongly displacing a valid
  older archive at the count cap. Excluding expired entries from capacity
  ranking made the same test pass.
- The staff test first reproduced successful reopening of an archive. Reopen
  and sticky now return an explicit invalid-action result; removal remains
  audited, and reports display read-only state. Public closed/sticky/settings
  mutation and protected-identity reads still fail with SQLSTATE 42501 and
  healthy allowed-operation controls.
- The public archive test first failed with 404 for an enabled-empty board.
  Both listeners now pass archive arrays, conditional responses, HEAD, archive
  fields, active-field omission, disabled/unknown/expired/deleted visibility,
  API-origin CORS, HTML escaping/navigation and archived reply rejection.
  Two old CORS negative tests were moved from the now-supported JSON archive
  route to the intentionally unsupported API HTML route.
- The real JavaScript-disabled browser workflow creates two OPs to cause
  rollover, navigates the archive, reports and password-deletes its archived
  OP, checks subsequent 404 and removes its owned fixture board.

`cargo test -p board-store -p board-staff --features database-tests --test archives --locked`
passed. `cargo test -p board-public --all-features --locked` passed 32 tests.
The dedicated archive browser test passed. Fixture cleanup uses owned board
identifiers and executes after captured test panics; it is not a promise of
cleanup after forced process/OS termination.

## Visual checks

Six new shared-template Windows baselines cover populated/empty archives and
archived thread pages at 1280x900 and 390x844. The synthetic populated archive
includes escaped markup and a maximum-length unbroken subject. Its initial mobile
check reproduced 805-pixel document width in a 390-pixel viewport. Scoped
archive-entry wrapping removed the overflow; all six final images were inspected
and strict replay passed. The three existing board/catalog baselines passed
unchanged. These fixtures establish project regressions, not original-site parity.

Run `npm run test:archive-visual` for the dedicated fixture suite. The ordinary
`npm run test:behavior` includes the actual persisted archive browser workflow.
The locked workspace example build supplies both fixture executables. Windows CI
runs old and archive visual suites in separate steps so a later successful native
command cannot mask an earlier failure.

## Migration, build and environment outcomes

The owned upgrade from 0008 passed: historical text/thread metadata preserved,
archives initially disabled, new public view accessible, expired entries hidden,
pinned archive mutation rejected, and unrelated public/staff/media grants denied.
Review then identified a cleanup window if grants failed after database creation.
The helper now records successful creation before executing grants, ensuring
its EXIT cleanup owns that database. A subsequent local qualifier run remained
live when the WSL environment stopped responding; it is not recorded as a pass.

Warnings-denied workspace Clippy, the locked workspace examples/binaries build,
formatting, actionlint and whitespace validation passed. Cargo-audit fetched
1,243 advisories and scanned 325 dependencies without a finding; npm audit found
zero vulnerabilities. No path-sensitive hosted advisory run is expected from
this change because its lockfiles and advisory workflow are unchanged.

The full Windows workspace test build initially failed with linker LNK1180,
insufficient disk space. An attempted incremental-directory cleanup was rejected
by automatic policy review; Cargo's narrower `cargo clean -p board-staff` succeeded
and removed 16.2 GiB of generated artifacts. The test retry compiled and began
execution, then failed in the existing media approval test with `PoolTimedOut`.
Ubuntu still reported running and accepted TCP connections, but SQL and new
process-inspection commands did not respond. Those observations establish a
local environment failure, not a passing full suite or a proven application
regression. Native qualification handles were retained rather than restarted
because an observation timeout is not process completion.

Final full-suite, migration, restore and platform outcomes are recorded on the
pull request after hosted CI. A local partial pass is not substituted for those
required checks.

## Review and limits

Separate storage and whole-branch source reviews found no remaining application
defects. The missing reply/rollover concurrency coverage and policy-shrink timing
were addressed. Whole-branch review identified the Windows exit-status masking
and migration cleanup window; both were corrected and accepted in scoped
re-review. Reviewers did not independently run the full suite.

Archives use bounded project retention/count policy and existing soft deletion.
Direct content credentials and backups retain underlying data; public expiry
does not physically erase it. Old binaries cannot safely serve archived rows
after migration. Public attachments, original-site visual/behavioral evidence,
production deployment/recovery/monitoring and independent deployed security
review remain open. See [operating notes](thread-archives.md) and
[readiness](readiness.md).
