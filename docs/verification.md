# Verification record

Collected September 8, 2026, in the current rewrite directory. This records a local text-board checkpoint. It does not establish production readiness or complete reference compatibility.

## Environment and provenance

- Windows NT 10.0.22621.0, Rust/Cargo 1.94.0, Node 25.2.1.
- PostgreSQL 16.15 in a separate disposable Ubuntu 24.04 WSL cluster, TCP port 55432. Tests use the actual `board_public` and `board_migrator` logins.
- Playwright 1.62.0 with Chromium 151.0.7922.34, revision 1234. Viewports, fonts and public API reference hashes are in [reference-manifest.json](reference-manifest.json).
- Fixture content is synthetic. The nested legacy checkout was excluded and left unchanged; its implementation was not used as reference material.

The root initially contained no rewrite source, repository instructions or attached design brief. The nested checkout's remote was inspected, but it is not a confirmed publication target for this rewrite. The authenticated GitHub account is `frankischilling`; the existing configured Git identity was checked against its verified email without changing global configuration.

## Commands and outcomes

Run PowerShell commands from the repository root. Database commands require `. .\.local\database.ps1`; this ignored file contains generated local secrets and must never be pasted into logs. The browser runner removes operator database variables from its public-server child environment.

| Command | Actual result |
|---|---|
| `rustup toolchain install 1.94.0 --profile minimal --component clippy --component rustfmt` | Installed the toolchain required by SQLx 0.9 |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/dev-db.sh` | Created a separate disposable cluster, roles and generated local environment; does not overwrite existing setup |
| `cargo run -p board-store --bin board-migrate --locked` | Applied the versioned schema using the migration login |
| `wsl -d Ubuntu -- bash -lc 'cd /mnt/c/Users/imike/4chan-rewrite && source .local/database.env && bash scripts/seed.sh'` | Loaded synthetic boards and fixed demonstration posts |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed with no warnings |
| `cargo build --workspace --locked` | Passed; development profile, not a production artifact deployment |
| `cargo test --workspace --all-features --locked` | 20 tests passed, none failed or ignored; includes real database integration tests |
| `npm.cmd ci --ignore-scripts` | Installed the locked test dependencies |
| `npm.cmd test` | 5 browser tests passed in the final run: persisted posting/reply/report/deletion; browsing/posting/deletion without JavaScript; three visual comparisons |
| `$env:VISUAL_FIXTURE_SERVER = '1'; npm.cmd run test:visual` | 3 visual comparisons passed against the test-only server using production templates |
| `cargo audit` | Passed; scanned 259 locked dependencies against 1,242 loaded advisories using cargo-audit 0.22.2 |
| `npm.cmd audit --audit-level=moderate` | Passed with zero reported vulnerabilities |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` | Passed: restored post fingerprint, eight table counts, public read and staff denial; generated restore database removed |
| `wsl -d Ubuntu -- bash -c 'for f in /mnt/c/Users/imike/4chan-rewrite/scripts/*.sh; do bash -n "$f" || exit; done'` | All five shell scripts passed syntax checks |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Exit 1, expected: reference, media, staff and deployed infrastructure prerequisites are missing |

The visual-fixture environment variable must be removed before normal browser testing. The full browser run uses the persisted application, not the fixture server. The final Windows browser run reported 7.1 seconds; this is a test-run duration, not an application performance measurement.

## Failures found and resolved

Development checks were not all green on their first run. The following failures led to corrections or explicit limits:

- Playwright 1.63.0 Chromium downloads timed out across the available mirrors. The project now pins the exact installed, matching Playwright 1.62.0/Chromium pair. No newer-browser result is claimed.
- The first screenshot run reported missing baselines. All three generated images were inspected before subsequent ordinary comparisons passed. No original reference screenshots were available; these are project regressions only.
- Browser posting initially failed because `Referrer-Policy: no-referrer` caused form requests to carry `Origin: null`. The policy is now `same-origin`, and the actual browser form workflow passes with strict origin checks intact.
- Review identified ambiguous PostgreSQL TLS query options, oversized preview reads and integer page overflow. Regression checks now cover rejected conflicting/alias TLS settings, SQL-bounded previews and extreme page values.
- `cargo test -p board-public --lib deletion_during_preview --locked` initially failed: subtracting one from a zero count produced an invalid unsigned reply count. Checked conversion now rejects a concurrently deleted thread; the test passes in the final suite.
- `cargo test -p board-store --all-features --locked thread_metadata_and_posts -- --nocapture` initially reproduced thread metadata with zero replies alongside one visible reply. A read-only repeatable-read transaction now supplies individual thread metadata and posts together; the bounded concurrent-write test passes. An earlier draft assertion incorrectly assumed post timestamps could not follow the thread timestamp within one transaction; it was removed because it did not express the snapshot property.

A bounded static review found the issues described above and rechecked the final fixes. It did not execute tests or review a deployed boundary. This was implementation assistance, not an independent security audit or a GitHub approval.

## What the tests establish

The public database login can read and mutate its permitted content, read deletion hashes, and insert reports. Actual SQLSTATE `42501` denials establish that it cannot read protected staff credentials, update staff roles, read deployment settings or reports, create a schema, or assume the migration role. Positive controls use the migration login to access the protected tables. The public startup check rejects the migration role. Soft-deleted content and deletion hashes remain readable by a compromised public database credential; this is documented retained authority.

Posting and deletion are persisted. Per-board transactions serialize limits, and eight simultaneous reply attempts cannot exceed the configured reply ceiling. Formatting properties use 128 bounded arbitrary-Unicode cases. HTTP checks exercise streamed body limits without Content-Length, forwarded-header rate-limit bypass attempts, absent storage, missing/invalid origins, invalid deletion passwords, unsupported uploads, headers, routes and the documented text-only JSON subset.

No media worker exists. There is no actual worker context from which to establish database, network, other-job or storage denial. No external worker CPU/memory/process/output/timeout enforcement has been tested. Enabling media fails startup; an absent worker is not evidence that the intended processing boundary is safe.

There is no staff session or WebAuthn implementation. Missing/expired/revoked staff session checks, enrollment/recovery, role administration, moderation and audit evidence remain unrun. The database schema denial tests do not substitute for them.

## Publication and unrun checks

The local branch is `feature/public-board-foundation`. All Git/GitHub operations use the installed Git Human Workflow helper, composed with Git Commit Author. At the initial local checkpoint, no remote or hosted publication existed. The user subsequently confirmed https://github.com/frankischilling/26chan as the destination. Publication preserves the original implementation commit and connects it to an empty review base; no release or deployment is involved.

The local results above do not establish hosted GitHub Actions results; consult the pull request checks for their current state. Linux application/browser behavior and Windows Server 2025 screenshot comparisons require separate verification. The local Windows fonts are recorded; a different runner image may require investigation, not automatic baseline replacement. No production release build, load test, live systemd resource test, network-policy test, durable backup isolation test, RPO/RTO exercise or independent security review has run.

Use [readiness.md](readiness.md) for the remaining acceptance work and [operations.md](operations.md) for local setup, restore limits and deployment prerequisites.
