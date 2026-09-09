# Dependency and update inventory

Exact Rust dependencies are pinned by `Cargo.lock`; core direct choices are Rust 1.94.0, Axum 0.8.9, Askama 0.16.1 and SQLx 0.9.0. SQLx 0.9 requires Rust 1.94. Official documentation and crate metadata were checked September 8, 2026: [Axum](https://docs.rs/axum/0.8.9/axum/), [Askama](https://docs.rs/askama/0.16.1/askama/), [SQLx](https://docs.rs/sqlx/0.9.0/sqlx/). PostgreSQL 16.15 remains in a [supported major release](https://www.postgresql.org/docs/16/backup-dump.html).

| Component | Pinned/tested version | Role and maintenance note |
|---|---|---|
| Tokio | 1.53.1 in lockfile | Async runtime; native OS integration and unsafe internals are in the trust base |
| Bytes / HTTP body | 1.12.1 / 1.1.0 | Direct declarations reuse existing locked versions. The local `board-http` crate retains admission through response data ownership; Bytes reference counting and its internal unsafe implementation remain in the trust base |
| Rustls / ring | 0.23.44 / 0.17.14 | Database TLS; ring includes native/assembly code. Keep certificate verification enabled |
| SQLx PostgreSQL driver | 0.9.0 | Bound SQL and PostgreSQL protocol. MySQL/SQLite packages can appear in the lock graph through macro metadata; no SQLite/MySQL driver is enabled in the public normal dependency tree |
| Askama | 0.16.1 | Compiled templates and automatic escaping; application never uses the `safe` filter |
| Argon2 | 0.5.3 | Local deletion passwords, default Argon2id parameters and random salts; four concurrent operations maximum |
| PNG / temporary files / randomness | 0.18.1 / 3.27.0 / getrandom 0.4.3 | Host media promotion invokes only the encoder; the separate guest invokes the PNG decoder. Compression, SIMD checksum and OS filesystem/randomness implementations remain trusted dependencies |
| WebAuthn / OpenSSL | webauthn-rs 0.5.5; vendored openssl-src 300.6.1+3.6.3 | Staff authentication only; native cryptography and authenticator-data parsing remain trusted dependencies. Server ceremony state is stored only in the protected database. No hardware attestation policy is claimed |
| Native staff build | Local Strawberry Perl 5.42.2.1 and MSVC; CI Perl and C build tools | Required to compile vendored OpenSSL. Portable Perl was checked against its published SHA-256; it is an ignored local build prerequisite, not a shipped application asset |
| URL / public suffix list | 2.5.8 / 2.1.231 | URL normalization, explicit schemes, origin/domain policy; update suffix data with tests |
| Playwright / Chromium | 1.62.0 / 151.0.7922.34, revision 1234 | Test-only browser, pinned to the installed matching pair. Attempted 1.63.0 download timed out; do not infer latest-browser coverage |
| Node | Local 25.2.1; CI 24.14.0 | Browser test runner only. Public pages need no JavaScript; staff ships a small local WebAuthn script |
| PostgreSQL | 16.15 | Disposable local database; production patching/backup verification still required |
| Host kernel | WSL 5.15.153.1, Ubuntu 24.04 userspace | Local testing only; not an approved processing host |
| Isolation runtime / guest | Firecracker and jailer 1.16.1; guest Linux 6.1.186; purpose-built Rust initramfs | Local qualification only; [artifact hashes and provenance](firecracker-artifacts.json). Upstream CI kernel is demonstration material; reviewed production host and guest rollout remain required |
| Guest syscall interface | rustix 1.1.4 / linux-raw-sys 0.12.1 | Safe first-party calls for guest initialization and resource limits; dependency syscall implementations use unsafe code. No existing registry dependency version changed |
| Workflow actions | checkout 7.0.1 (`3d3c42e5aac5ba805825da76410c181273ba90b1`); setup-node 7.0.0 (`820762786026740c76f36085b0efc47a31fe5020`) | Node 24 action runtimes; `contents: read`, no persisted checkout credential or automatic package-manager cache; no pull_request_target execution |

First-party `forbid(unsafe_code)` does not apply to dependencies. The normal public dependency tree includes ring, Rustls, Tokio/mio/socket2 and Windows system bindings; these need maintenance even though complex media parsing is absent. A complete transitive unsafe-code audit has not been performed.

Durable media publication uses Rust 1.94's standard-library file locks, existing SHA-256/PNG code and directory synchronization on Unix. These operating-system implementations are part of the publication trust base. The new direct Serde declaration reuses the existing locked version; no registry dependency version changed. Windows tests cover development behavior, not directory-entry durability under power loss.

The local media setup additionally depends on Python 3.12, systemd 255, GNU coreutils `timeout` 9.4, mount/umount, KVM, tmpfs, and effective memory/CPU/pids cgroup controllers. These belong to the host trust base. The launch monitor and service client must retain the inherited coordinator lock; [recovery tests](media-recovery.md) verify that behavior and the independent monitor deadline after parent SIGKILL. The per-job VM receives no Python, shell, package manager or network device. Patch the pinned kernel/runtime and rebuild both init and worker before re-running the [local qualification checks](firecracker.md); successful CI on one host is not approval of another processing tier.

The scheduled advisory workflow runs cargo-audit 0.22.2 and npm audit weekly. The operator must subscribe to failures and triage them; no alert delivery has been configured. A clean advisory result only covers known entries at the fetched revision.

The action pins were resolved from the official [checkout 7.0.1 release](https://github.com/actions/checkout/releases/tag/v7.0.1) and [setup-node 7.0.0 release](https://github.com/actions/setup-node/releases/tag/v7.0.0) on September 8, 2026. Both actions require a runner supporting Node 24 (minimum 2.327.1); the configured hosted runners supply it. The workflow still installs Node 24.14.0 for browser tooling. Automatic package-manager caching is explicitly disabled because newer setup-node releases enable it for detected npm projects. [Verification notes](verification-ci-actions.md) record local checks and distinguish hosted checks awaiting execution for issue #10.

For updates, open a focused change that records affected components and advisories, update exact tool/lock versions, run formatting/clippy/unit/database/browser checks, and inspect any screenshot differences. Re-run restore tests after database or migration changes. For future media updates, rebuild disposable guests and repeat connectivity/resource/promotion tests before enabling them. Do not blanket-refresh baselines or silently waive findings. Production updates require the operator's deployment approval.
