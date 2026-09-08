# Dependency and update inventory

Exact Rust dependencies are pinned by `Cargo.lock`; core direct choices are Rust 1.94.0, Axum 0.8.9, Askama 0.16.1 and SQLx 0.9.0. SQLx 0.9 requires Rust 1.94. Official documentation and crate metadata were checked September 8, 2026: [Axum](https://docs.rs/axum/0.8.9/axum/), [Askama](https://docs.rs/askama/0.16.1/askama/), [SQLx](https://docs.rs/sqlx/0.9.0/sqlx/). PostgreSQL 16.15 remains in a [supported major release](https://www.postgresql.org/docs/16/backup-dump.html).

| Component | Pinned/tested version | Role and maintenance note |
|---|---|---|
| Tokio | 1.53.1 in lockfile | Async runtime; native OS integration and unsafe internals are in the trust base |
| Rustls / ring | 0.23.44 / 0.17.14 | Database TLS; ring includes native/assembly code. Keep certificate verification enabled |
| SQLx PostgreSQL driver | 0.9.0 | Bound SQL and PostgreSQL protocol. MySQL/SQLite packages can appear in the lock graph through macro metadata; no SQLite/MySQL driver is enabled in the public normal dependency tree |
| Askama | 0.16.1 | Compiled templates and automatic escaping; application never uses the `safe` filter |
| Argon2 | 0.5.3 | Local deletion passwords, default Argon2id parameters and random salts; four concurrent operations maximum |
| URL / public suffix list | 2.5.8 / 2.1.231 | URL normalization, explicit schemes, origin/domain policy; update suffix data with tests |
| Playwright / Chromium | 1.62.0 / 151.0.7922.34, revision 1234 | Test-only browser, pinned to the installed matching pair. Attempted 1.63.0 download timed out; do not infer latest-browser coverage |
| Node | Local 25.2.1; CI 24.14.0 | Browser test runner only; no JavaScript shipped to page users |
| PostgreSQL | 16.15 | Disposable local database; production patching/backup verification still required |
| Host kernel | WSL 5.15.153.1, Ubuntu 24.04 userspace | Local testing only; not an approved processing host |
| Isolation runtime / guest images / decoders | None installed for this project | Media disabled. Pin and inventory the runtime, host/guest kernels, rootfs and tools when implemented |
| Workflow actions | Full commit IDs in workflows | `contents: read`; no persisted checkout credential or pull_request_target execution |

First-party `forbid(unsafe_code)` does not apply to dependencies. The normal public dependency tree includes ring, Rustls, Tokio/mio/socket2 and Windows system bindings; these need maintenance even though complex media parsing is absent. A complete transitive unsafe-code audit has not been performed.

The scheduled advisory workflow runs cargo-audit 0.22.2 and npm audit weekly. The operator must subscribe to failures and triage them; no alert delivery has been configured. A clean advisory result only covers known entries at the fetched revision.

For updates, open a focused change that records affected components and advisories, update exact tool/lock versions, run formatting/clippy/unit/database/browser checks, and inspect any screenshot differences. Re-run restore tests after database or migration changes. For future media updates, rebuild disposable guests and repeat connectivity/resource/promotion tests before enabling them. Do not blanket-refresh baselines or silently waive findings. Production updates require the operator's deployment approval.
