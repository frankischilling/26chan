# Staff inactivity verification

This checkpoint extends compatibility requirement B-007 with project-defined staff security policy. It adds a configurable inactivity deadline, preserves absolute expiry and recent WebAuthn authentication, and applies migration 0006 with a narrow activity-column grant. It changes no public HTML, JSON fields, media behavior or screenshot baseline.

## Environment and commands

Verified in the Windows workspace with Rust 1.94.0, the locked dependency set, Chromium 151.0.7922.34 through Playwright 1.62.0, and PostgreSQL 16.15 in the disposable Ubuntu WSL cluster. The three ignored `.local/*.ps1` credential files supplied separate migration, public, media, authentication and moderation logins; no credentials were committed. Native staff builds used the per-command `OPENSSL_SRC_PERL` path documented in [staff operation](staff.md).

Commands below ran against the same source and migration state. No screenshot baseline was refreshed.

| Command | Actual outcome |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo build --workspace --examples --bins --locked` | Passed |
| `cargo test --workspace --all-features --locked` | Passed: 78 tests, zero failed or ignored |
| `cargo test -p board-staff --features database-tests --locked` | Passed: 19 focused staff tests, including seven database inactivity checks and startup validation |
| `cargo run -p board-store --bin board-migrate --locked` | Applied migration 0006 to the disposable development database |
| `npm.cmd test` | Passed: five public behavior/API tests and three unchanged synthetic screenshot baselines |
| `npm.cmd run test:staff` | Passed: one real browser enrollment/login/moderation/recovery flow, including activity extension and idle denial |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/test-staff-idle-migration.sh` | Passed. Applied actual migrations 0001 through 0005 in a new database, inserted old and live sessions, applied 0006, checked exact historical deadlines and fresh activity defaults, then removed the database |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` | Passed. Post fingerprint and fourteen table counts matched; restored public/media/auth/staff reads, activity-column permissions and protected-operation denials passed. Restored database removed |
| `cargo audit` | Passed: 310 locked dependencies checked against 1,242 advisories |
| `npm.cmd audit --audit-level=moderate` | Passed: zero reported vulnerabilities |
| `node --check tests/browser/staff.spec.js` and `node --check playwright.staff.config.js` | Passed |
| `.\.local\tools\actionlint.exe .github/workflows/ci.yml .github/workflows/advisories.yml` | Passed with actionlint 1.7.12, installed only under ignored `.local/tools` |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1: reference evidence, deployed media containment, production staff policy and deployed operations remain unverified |

## Scope and deployment limits

The initial inactivity tests failed with PostgreSQL `42703` before the activity column existed. A later regression also failed when a held row lock crossed the activity deadline: checking expiry only in an `UPDATE` could use a predicate evaluated before the lock wait. The final implementation locks the session row first, then checks live identity and both deadlines before writing activity within the same transaction. The regression observes an actual blocked `board_auth` backend and waits for database time to cross the deadline. Its final denial preserves activity. No broader identity-table update permission was added.

A separate source review found no blocking findings. Three additional regression cases remain useful: revocation or role/credential change during an observed lock wait, an explicit comparison of cookie value and expiry before/after activity, and calling readiness against the old schema. The relevant paths were reviewed, but those exact scenarios were not separately executed. Existing tests cover live revocation, persisted deadlines, real idle denial/login and the historical migration; review does not establish a deployed security boundary.

The browser fixture moves timestamps only through its offline migration identity. There is no runtime test-authentication or clock override. The WebAuthn browser uses a virtual authenticator; physical hardware protection, attestation and production enrollment/recovery policy remain unverified. A deployed timeout must be selected through the operator's risk review; the default and allowed range are project decisions.

All staff instances must use the same timeout. Stop staff serving and invalidate existing sessions before changing it. Stop old staff binaries before applying migration 0006, and restart with the new binary after migration. An old binary would remove inactivity enforcement; retaining the database column alone is insufficient. [Staff instructions](staff.md) describe the rollout and rollback restriction.

No media worker was introduced or run. There is no new evidence about what a compromised worker can reach or consume. Public media enablement remains rejected; isolated execution, external resource enforcement, network denial, healthy positive controls and deployed publication qualification remain prerequisites in [issue #5](https://github.com/frankischilling/26chan/issues/5). Permitted visual/behavioral reference evidence remains unresolved in [issue #6](https://github.com/frankischilling/26chan/issues/6). No production deployment or full 1:1 compatibility claim is made.
