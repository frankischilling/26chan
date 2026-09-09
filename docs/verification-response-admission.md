# Response admission verification

This checkpoint extends the project-defined concurrency policy in compatibility exception E-006. Public/API capacity remains 32 and staff capacity remains 16. A request retains admission through its handler, final response body and emitted application data, including shared data clones and slices. Public/API overload remains 503; staff overload remains 429. Page content, routes, authentication, database permissions and screenshot baselines are unchanged.

## Implementation and scope

The shared `board-http` utility carries an owned semaphore permit in a private response extension until body-rewriting middleware has finished. An outermost body layer removes that extension and transfers ownership to the final body. Emitted nonempty `Bytes` frames carry the same permit through `Bytes::from_owner`. This prevents dropping the body handle from releasing admission while its data remains held elsewhere. Empty final responses release immediately; completed or failed bodies release their own hold while previously emitted data retains its hold. Dropping an unconsumed response or cancelling a handler frees its owned resources.

The public/API services share one semaphore, including API responses normalized by CORS. Staff uses the same utility with its separate semaphore and private/security headers. Handler deadlines remain ten seconds. No migration is needed.

This is a count limit on admitted work and retained application response data. It is not a total memory/byte bound, a socket connection limit, a response-write deadline or proof that the client received the response. Small overload responses, kernel buffers and copies made by downstream consumers are outside the count. Slow clients can occupy slots; proxy connection/header/write timeouts, aggregate byte budgets and external resource/load qualification remain prerequisites.

## Environment and results

Verification uses the Windows workspace, Rust 1.94.0, PostgreSQL 16.15 in the disposable Ubuntu WSL cluster, Playwright 1.62.0 and Chromium 151.0.7922.34. The ignored `.local/database.ps1`, `.local/media.ps1` and `.local/staff.ps1` files provide distinct test credentials. Native staff builds use the local Perl path through `OPENSSL_SRC_PERL`.

| Command | Actual outcome |
|---|---|
| `cargo test -p board-public --test http_limits retained_public_responses_keep_the_admission_budget --locked` before implementation | Expected failure: the 33rd request returned 200 instead of 503 while 32 completed response bodies were retained |
| `cargo test -p board-staff --test http retained_staff_responses_keep_the_admission_budget --locked` before implementation | Expected failure: the 17th request returned 200 instead of 429 while 16 completed response bodies were retained |
| `cargo test --offline --locked -p board-http` | Passed: 13 ownership tests. The prior no-op stub ran 13 tests with 12 expected failures and one unchanged-response case passing |
| `cargo test -p board-public --features database-tests --test http_limits --test api_cors --locked` | Passed: 16 router tests, including admission, CORS, transformed/empty bodies and actual chunked transport limits |
| `cargo test -p board-staff --test http --locked` | Passed: four HTTP tests, including retained body/data admission and existing authorization/attempt limits |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo build --workspace --examples --bins --locked` | Passed |
| `cargo test --workspace --all-features --locked` | Passed: 101 tests, zero failed or ignored |
| `npm.cmd test` | Passed: five public/API browser checks and three unchanged screenshot baselines |
| `npm.cmd run test:staff` | Passed: real WebAuthn enrollment, login, moderation, recovery and idle-session flow |
| `cargo audit` | Passed: 311 locked packages checked against 1,243 advisories |
| `npm.cmd audit --audit-level=moderate` | Passed: zero reported vulnerabilities |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/test-comment-migration.sh` | Passed: historical text/settings/grants preserved, full Unicode runtime-role insert and LATIN1 denial; generated databases removed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/test-staff-idle-migration.sh` | Passed: historical session deadlines and fresh activity defaults preserved; generated database removed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` | Passed: post fingerprint and fourteen table counts matched; restored public/media/auth/staff reads, activity-column grants and protected-operation denials passed; generated database removed |
| `.\.local\tools\actionlint.exe .github/workflows/ci.yml .github/workflows/advisories.yml` | Passed |
| `node --check tests/browser/behavior.spec.js` | Passed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1: permitted reference evidence, media containment, production staff policy and deployed operations remain unverified |

The router regressions use tiny harmless response bodies and real Axum routing. They check retained response/data ownership, recovery after release, shared public/API capacity, API error-body replacement, empty HEAD/OPTIONS transformations and cancellation of handlers waiting on a controlled loopback database listener. The existing database and real-browser suites exercise normal operation through the changed middleware. These are application ownership checks, not an external saturation benchmark.

The implementation reuses `bytes` 1.12.1 and `http-body` 1.1.0 already in the lockfile. Their locally installed source contracts were inspected: `Bytes::from_owner` retains its owner until the remaining clones are dropped, and `http_body::Body` defines frame, error and end-of-stream behavior. Online documentation fetches for those exact versions were unavailable; the locked local sources and executed tests supply the version-specific evidence. The registry package name/version/source/checksum list is unchanged; only the local crate and app dependency edges were added.

## Review, rollout and remaining work

A separate source review found no issues requiring changes. Hosted Linux and Windows results will be recorded on the draft PR.

Use the existing [local setup](../README.md) and credentials. This change requires no new configuration, database migration or public/staff permission. Restart both applications with the updated binaries; instances running old binaries keep the shorter admission lifetime. Production still needs qualified proxy connection/header/write limits and external resource ceilings.

No media worker was introduced or run. The environment inventory confirmed an available WSL `/dev/kvm` and mounted cgroup controllers; that establishes no worker isolation or quota property. Firecracker, guest artifacts and a deployed processing tier remain unqualified. Public media enablement stays rejected, and worker filesystem/network/credential/resource containment tests remain unrun under [issue #5](https://github.com/frankischilling/26chan/issues/5). Permitted visual/behavioral reference evidence remains unresolved under [issue #6](https://github.com/frankischilling/26chan/issues/6). Production staff policy, hardware authenticators, backup isolation, monitoring and independent deployed review remain open. No production deployment or release was attempted.
