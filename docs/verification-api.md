# API origin verification

This continuation began September 8, 2026, from merged `origin/main` at `980195b8f8fb1261af667561a4b9303d78c12e13`, in the requested checkout. Inventory found no user changes or applicable AGENTS.md. GitHub authentication resolved to `frankischilling`; the configured Git email matched that account's verified primary email. No global identity settings were changed. Work uses branch `feature/api-origin-compatibility` and [issue #8](https://github.com/frankischilling/26chan/issues/8).

The API route checkpoint is commit `c469b53`; commit `03d16b1` adds listener startup and the review fixes. The read-only API README was collected at pinned revision `2bd670d507ba2daa37a3961a661e088cf6f89d57`; its SHA-256 is recorded in the reference manifest and matches the downloaded file. The nested `4chan-old` checkout's remote and revision were inventoried, but its implementation was not imported or run. Its provenance remains unresolved under the supplied prompt's exclusion of leaked/private source. No separate design-brief attachment or permitted visual/interactive snapshot was available.

## Environment and commands

Local checks use Windows, Rust 1.94.0, Node 25.2.1, Playwright 1.62.0 and Chromium 151.0.7922.34. Actual PostgreSQL 16.15 runs in the disposable WSL cluster on port 55432. Tests load the generated, ignored `.local/database.ps1`, `.local/media.ps1` and `.local/staff.ps1` as needed. Windows native staff builds use the existing portable Perl through `OPENSSL_SRC_PERL`; credentials are never printed or committed.

| Command | Observed outcome |
|---|---|
| `cargo test --workspace --all-features --locked` before edits | 56 passed; no failed or ignored tests |
| `cargo test -p board-public --test startup invalid_api_listener_configuration --locked` | Expected failure: incomplete API settings were ignored before database access |
| `cargo test -p board-public --test startup occupied_api_listener --locked` | Expected failure: API bind was not attempted before database connection |
| `cargo test -p board-config --locked` | Six passed after API validation implementation |
| `cargo test -p board-public --test startup --locked` | Five passed, including pair validation and occupied-port rollback |
| `cargo fmt --all -- --check` | Initially identified formatting differences; passed after rustfmt |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed before and after review fixes; final formatter check also passed |
| `cargo build --workspace --examples --bins --locked` | Passed before and after review fixes |
| `cargo test --workspace --all-features --locked` after integration | 65 passed before review fixes; final run: 69 passed, no failed or ignored tests |
| `node --check tests/browser/behavior.spec.js` and `node --check playwright.config.js` | Passed |
| `npm.cmd run test:behavior` | First run: three new client checks failed on Chromium loopback permission, two existing tests passed. After an origin-scoped test permission: all five passed |
| `npm.cmd run test:visual` with `VISUAL_FIXTURE_SERVER=1` | Three unchanged Windows baselines passed |
| `npm.cmd test` using the real database-backed application | Eight passed before and after review fixes: five behavior checks and three unchanged screenshots; final cache scenario covers both conditional request headers |
| `npm.cmd run test:staff` | One passed before and after review fixes: synthetic WebAuthn enrollment/login, persisted moderation/audit, recovery and logout |
| `cargo audit` | Passed against 310 locked dependencies and 1,242 loaded advisories |
| `npm.cmd audit --audit-level=moderate` | Passed; zero reported vulnerabilities |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh` | Passed post fingerprint, fourteen table counts, restored runtime reads and protected-operation denials; disposable restore database removed |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1: reference, media containment, production policies and review prerequisites remain incomplete |

## Review and limits

Review identified two configuration/contract defects: a production API IP literal could pass the DNS check, and shared protection errors could remain plain text on the JSON listener. Both had failing regressions before their fixes. Production origin validation now requires a URL DNS host before suffix lookup. The API adapts protection errors to stable JSON while retaining status and headers and preserving bodyless HEAD/OPTIONS/304 responses.

Added tests exercise shared public/API admission, busy OPTIONS rejection, representative preflight/error security headers including HSTS, existing Vary preservation, and paired-listener shutdown. The shutdown fixture initially hung because two same-typed Extension values collided and its reads blocked a single-threaded test runtime. Distinct state fields, bounded socket waits and blocking I/O tasks corrected the fixture. The final test drives active requests on both listeners, checks that their responses finish, then rebinds both sockets. Only the identified hung test processes were terminated during debugging.

Focused fix checks were `cargo test -p board-config` (six passed), `cargo test -p board-public --features database-tests` (23 passed), strict Clippy for those packages, and the targeted Playwright cache scenario (one passed). The complete locked workspace and browser commands in the table then passed again. A separate local code-review pass approved both fixes. Its remaining nonblocking coverage notes are the full header set on denied implemented-path preflights and a dedicated Retry-After preservation regression; those behaviors were inspected in code but are not separately demonstrated by those assertions.

The browser tests grant local-network permission only to their synthetic loopback client origins. This is separate from CORS, which remains active. Positive controls read the same real API before denied-origin tests. Production CSP and all screenshot baselines remain unchanged.

No media worker was run. No statement about a compromised worker's connectivity, credentials, storage access or resource ceilings is established by this checkpoint. `/dev/kvm` exists in WSL, but no reviewed processing host, guest or deployed coordinator is configured. Public uploads stay disabled. The restored database exercise establishes disposable recovery and grants, not production backup deletion isolation or recovery targets.

The optional API shares the public process's authority. Actual PostgreSQL tests continue to deny that login access to protected staff and deployment data. HTTPS proxy host routing, production network policies and service-level ceilings have not been deployed or tested. The overall rewrite remains incomplete. See [API setup](api.md), [compatibility](compatibility.md) and [launch prerequisites](readiness.md).
