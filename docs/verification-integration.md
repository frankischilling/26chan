# Reviewed slice integration

On September 10, 2026 UTC, integration brought together authenticated media
dispatch and candidate service qualification (PRs #23/#25), consistent public
board and thread responses (#24/#27), and documented board-return posting
options (#29). The user authorized merging after implementation and passing CI.
No production release or deployment is part of this checkpoint.

PR #29 merged into main as `bd811db`. The two reviewed follow-ups first merged
into their parent branches: #25 as `b3f6a62`, and #27 as `5f2d5c2`. Both had
passing Linux and Windows PR and push checks at their reviewed heads. The
snapshot branch incorporated main as `d206d25`. Media dispatch incorporated
main as `023a4d3`, resolving only a readiness-document conflict by retaining
the FAQ provenance row and the dispatch qualification row.

The final local integration merged `d206d25` into `023a4d3` without source
conflicts. Separate read-only reviews found no findings in the five PR ranges,
the documentation resolution, or the combined source. The combined public/store
source retained both posting options and snapshot transactions; media dispatch
source was unchanged. The reviews did not execute tests.

## Local verification

Commands ran in the Windows workspace with Rust 1.94.0 and the existing
disposable PostgreSQL environment. The ignored database, media, reader and staff
PowerShell environment files were sourced privately. Vendored OpenSSL used
`.local/strawberry-perl/perl/bin/perl.exe` through `OPENSSL_SRC_PERL`. Browser
tests cleared `VISUAL_FIXTURE_SERVER` and used the real database-backed apps.

| Command | Observed outcome on the combined tree |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo build --workspace --examples --bins --locked` | Passed |
| `cargo test --workspace --all-features --locked` | 144 passed, none failed or ignored |
| `npm.cmd test` | Six behavior checks and three unchanged screenshots passed in 18.0 seconds |
| `npm.cmd run test:staff` | WebAuthn/moderation/recovery flow passed in 7.2 seconds |
| `cargo audit` | 324 locked dependencies checked against 1,243 advisories; passed |
| `npm.cmd audit --audit-level=moderate` | Zero reported vulnerabilities |
| `git diff --check` | Passed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1; launch prerequisites remain incomplete |

Node emitted its existing `NO_COLOR`/`FORCE_COLOR` warning during the staff
browser run. PowerShell formatted stderr as `NativeCommandError`; the process
exited zero and the test passed. No assertion or screenshot baseline changed.
The preceding media-plus-posting tree also passed the full local Rust suite.

Hosted checks must be inspected at the final PR head before merging. Earlier
passing runs do not qualify a changed tree. PR descriptions retain the relevant
run links; Linux CI executes migrations, database permissions, browser behavior,
actual disposable media guests and recovery, and the restore exercise. The
dispatch workflow also executes direct and candidate-service dispatch. Windows
CI checks the pinned synthetic screenshots.

## Remaining scope

This checkpoint changes no database schema or grant and introduces no new
registry dependency beyond the reviewed feature branches. Local run commands
remain in the [README](../README.md); dispatch prerequisites and qualification
scope remain in [media dispatch](media-dispatch.md).

The worker evidence establishes only the operations exercised on recorded owned
test profiles, with healthy allowed-context controls. It does not establish a
production host boundary. Public uploads stay disabled. Separate media HTTP
serving and attachment, deployed credentials/network/storage/resource policies,
certificate operations, power-loss recovery, hardware staff authentication and
independent deployed review remain open. Permitted original visual/behavioral
reference evidence and complete compatibility also remain unresolved under
issues #5 and #6. See [readiness](readiness.md) for the full launch prerequisites.
