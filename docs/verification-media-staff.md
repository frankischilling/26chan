# Media and staff verification checkpoint

Work began September 8, 2026, from clean `main` at `8f3cdb4265630d56cc547ac6d11237474f6c1dd9` in `C:\Users\imike\4chan-rewrite`. The configured origin is `https://github.com/frankischilling/26chan.git`. GitHub authentication and the configured author were checked against the account's verified email. No global identity settings were changed. The earlier public foundation is merged in PR #2; this work is on `feature/media-quarantine`, tracked by issues [#3](https://github.com/frankischilling/26chan/issues/3) and [#4](https://github.com/frankischilling/26chan/issues/4).

The runtime environment remains Windows with Rust 1.94.0, Node 25.2.1 and the pinned Playwright 1.62.0 / Chromium 151.0.7922.34. Actual database tests use the separate PostgreSQL 16.15 disposable WSL cluster on port 55432. The existing public API reference revision and three synthetic screenshot baselines were preserved. No original visual/interactive reference or separate design brief was found.

The supplied `4chan-old` directory is a separate, ignored checkout. Its top-level inventory, README and license text were inspected; this does not establish the provenance of the underlying implementation or a permitted reference snapshot. Its runtime, private configuration and code were not used to implement or run this checkpoint. The prompt explicitly excludes leaked/private source. A permitted reference collection is still required before any 1:1 claim.

## Decisions made during this checkpoint

| Decision | Basis and cost of revisiting it |
|---|---|
| Use the current checkout on a feature branch | The prompt requests the current directory. If isolation is later required, move the branch work to a separate checkout. |
| Use the prompt and existing architecture without a separate brief | No separate brief was available. Reconcile any different requirements when its location is supplied. |
| Build private media plumbing while leaving public processing disabled | No reviewed processing tier or guest exists. The selected tier may require protocol/integration changes before enablement. |
| Proceed with staff independently and keep role administration/recovery offline | This completes independent work while media is blocked and keeps account administration outside web-runtime grants. A different operator policy would require workflow changes and another review before deployment. |

## Commands and observed outcomes

Database commands run from PowerShell after `. .\.local\database.ps1` and `. .\.local\media.ps1`; staff tests also need `. .\.local\staff.ps1`. These files contain generated secrets and must not be printed or committed. Individual runtime processes reject unrelated database credentials; test launchers strip them before spawning services.

| Command | Observed outcome |
|---|---|
| `cargo test --workspace --all-features --locked` before changes | 20 passed, 0 failed/ignored |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/dev-media-db.sh` | Created actual separate development media login and private credential files |
| `cargo run -p board-store --bin board-migrate --locked` | Applied media schema and queued-job expiration migrations |
| `cargo test -p board-store --features database-tests --test media_queue --locked` | Two passed: actual role denials and admission/claim/lease/retry/cleanup |
| `cargo test -p board-media --locked` | 19 passed, including real streaming/filesystem and bounded property tests |
| `cargo test -p board-media-admin --features database-tests --test cli --locked` | Two passed: isolated command configuration and real persisted intake/status/cleanup |
| `cargo test -p board-media-admin --features database-tests --test pipeline --locked` | One passed: synthetic bytes connect private intake, queue, output validation and idempotent receipts; no worker executes |
| `cargo test -p board-public --test startup --locked` | Three passed, including media credential inheritance rejection |
| `cargo clippy -p board-store -p board-config -p board-media-admin --all-targets --all-features --locked -- -D warnings` | Passed after replacing a complex tuple with a named receipt record |
| `cargo test -p board-media -p board-store -p board-config -p board-media-admin -p board-public --all-features --locked` | 41 passed, 0 failed/ignored; domain's four tests were not selected by this command |
| `cargo test -p board-staff --features database-tests --locked` | Ten passed: four unit, three actual PostgreSQL, three HTTP |
| `cargo fmt --all -- --check` | Passed on the combined workspace |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed on the combined workspace |
| `cargo build --workspace --examples --bins --locked` | Passed, including staff/public browser helpers |
| `cargo test --workspace --all-features --locked` after integration | 56 passed, 0 failed/ignored |
| `npm.cmd test` | Five passed: persisted public actions, no-JavaScript actions and three unchanged Windows screenshots |
| `npm.cmd run test:staff` after the final challenge-test correction | One passed: real WebAuthn virtual-authenticator enrollment/login, protected moderation/audit, recovery/revocation/logout, challenge/session failures and public ETag invalidation; 7.0 seconds total |
| `cargo clippy -p board-staff --example browser-fixture --locked -- -D warnings` | Passed after the final fixture-only change; whole-workspace formatter check also passed again |
| `cargo audit` | Passed against 310 locked packages and 1,242 loaded advisories |
| `npm.cmd audit --audit-level=moderate` | Passed; zero reported vulnerabilities |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` before staff migration | Passed post fingerprint, ten table counts and actual restored public/media allowed queries and protected-data denials; generated restore DB removed |
| Same restore command after final staff tests and migrations 0001–0005 | Passed post fingerprint, fourteen table counts, all four runtime positive reads and protected-operation denials; generated restore DB removed |
| Shell syntax checks over `scripts/*.sh` | Passed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1; deployed containment, production operations and reference requirements remain incomplete |

## Failures and corrections

The tests were not green throughout implementation. The first media database test failed because its credential and schema did not exist. After real provisioning it failed on missing `media.queue_policy`, then passed after migration. Queue API tests initially did not compile because the module was absent. SQLx 0.9 also rejected dynamically formatted SQL; the implementation now uses static query strings and bound values. A queued-expiration test exposed a state constraint mismatch, corrected in a separate migration.

The operator startup tests initially failed against the command skeleton; production and inherited-credential checks now run before intake. Clearing the entire environment in a Windows test child prevented database connectivity; preserving only `SystemRoot` restored the required OS setup without passing application secrets. The synthetic pipeline test had an invalid `format!` expression that was corrected before it ran. Strict Clippy identified a complex completion tuple; a named structure resolved it without suppressing the diagnostic.

The first public browser run during staff dependency editing could not start because `--locked` detected a stale lockfile. After the staff dependency fetch updated the lock, all five public browser tests passed. No screenshot baselines were refreshed. Initial GitHub inventory commands also needed quoted comma-separated JSON field lists in PowerShell; subsequent reads verified the repository and merged PR.

A new regression showed that distinct production origins on different ports could still share the public/staff cookie hostname. The shared configuration now rejects that arrangement; `cargo test -p board-config --locked` passed all four tests after the correction. Loopback development exceptions remain explicit. Review also strengthened both media output properties to supply trailing body bytes and assert `InvalidOutput` after exactly sixteen consumed bytes; `cargo test -p board-media --test output --locked` passed all ten tests.

Staff's initial native build failed without OpenSSL. Vendored OpenSSL and a verified portable Perl prerequisite resolved it; no system-wide install was needed. Askama compilation exposed missing macro terminators and slice borrowing errors, which were corrected. The first staff browser startup also failed on unquoted Windows executable paths; quoted absolute paths resolved that failure. Strict Clippy later required the configuration test module to follow production items.

Actual authentication-role access initially allowed account role updates under the foundation's reserved grants. Migration 0004 removes that authority. Temporary local startup tests then checked rejection of added role membership, database ownership and deployment schema access. Restoring database ownership in the first test also removed the authentication role's explicit CONNECT grant; a healthy control caught this fixture error. The grant and fixture cleanup were corrected, all three checks were rerun, and subsequent real login/database/browser tests passed. No temporary grants remain.

Staff database URL tests failed before strict login, query-option and verified-TLS validation was implemented. Review also found that the pinned passkey library can legitimately upgrade backup eligibility. A real-role regression failed under migration 0004; new migration 0005 permits that one-way metadata change while preserving immutable keys and rejecting downgrade. The corrected regression passes.

Adding migration 0005 initially left the existing SQLx migrator binary unchanged, so the regression stayed red after a nominal migration run. An explicit rebuild (`cargo rustc -p board-store --bin board-migrate --locked -- --cfg staff_migration_refresh`) followed by running `board-migrate.exe` applied it. The permanent fix is `crates/store/build.rs`, which tracks the migration directory as recommended by [SQLx's stable-Rust guidance](https://docs.rs/sqlx/0.9.0/sqlx/macro.migrate.html). After a normal build, adding a harmless non-SQL file produced Cargo's `Dirty board-store` and an actual successful recompile under `cargo build -p board-store --bin board-migrate --locked -vv`. The probe was removed; only the five intended migrations remain.

## What this evidence establishes

The media login can operate queue metadata but actual PostgreSQL denials prevent access to content, deletion hashes, staff identities and deployment settings, capacity changes, schema creation and migration-role assumption. The public login cannot access the media schema. Concurrent admission enforces capacity and claims are exclusive. Expired/replaced tokens fail completion; retries terminate. Input streaming stops at the ceiling plus one lookahead byte and failed/cancelled intake leaves no partial object in the tested filesystem. Generated PNG promotion is capped and no-clobber; exact replay is idempotent.

There is no actual media worker security context. Worker network/credential/other-job/storage denials, external CPU/memory/disk/process/runtime enforcement, full guest termination and contaminated-workspace disposal have not run. The library/queue tests cannot answer what a compromised deployed worker can reach. Public processing remains disabled. A production publication adapter must fence storage writes and reconcile database/filesystem crash windows. [Media notes](media.md) record these limits and the required owned-environment checks.

Local implementation reviews of the media storage and queue found no blocking issues within their stated development scope. The optional property-test observation was corrected and the reviewer confirmed the new byte-accounting assertions. These reviews are implementation assistance, not independent security audits or GitHub approvals.

The staff review's credential-update finding was corrected in migration 0005 and re-reviewed. Final review also confirmed typed TLS/identity configuration, production cookie-host separation, migration-directory build tracking and service-launch integration. Browser challenge evidence was strengthened: replay restores the original handle and assertion; expiry targets the exact currently signed challenge, with one affected database row required. The final reviewer confirmed both corrections and recorded no unresolved observations.

The lead ran the combined 56-test Rust suite, both browser suites, strict build/lint checks, audits and final restore shown above. Production deployment, hardware authenticator verification, microVM containment, durable backup isolation, recovery timing and original visual parity remain unverified. Issues [#5](https://github.com/frankischilling/26chan/issues/5) and [#6](https://github.com/frankischilling/26chan/issues/6) track the processing tier and permitted compatibility reference.

## Published checkpoint

Branch `feature/media-quarantine` was pushed to the existing origin and [draft PR #7](https://github.com/frankischilling/26chan/pull/7) was opened against `main`. The implementation commits are `d8b7e4e` (storage), `f13047f` (staff), `db6408d` (challenge evidence), and `99de512` (queue/shared integration). All four use the verified configured author and committer. This record and plan completion are a subsequent documentation commit.

Hosted results are shown in the [PR checks](https://github.com/frankischilling/26chan/pull/7/checks) and recorded in its description after completion; the local results above do not imply a hosted pass. The PR remains a draft for review. No merge, release or production deployment was performed.

The first hosted run on documentation head `9528425` passed Windows screenshots and dependency advisories, but Linux stopped at strict Clippy: the Unix-only invitation-file branch had a needless `return`. Windows does not compile that branch. The correction uses the block's tail expression and leaves the lint enabled; subsequent hosted runs verify the Unix build and the remaining checks. Earlier runs on superseded head `99de512` were cancelled after the newer runs started; no current-head required check was cancelled or waived.
