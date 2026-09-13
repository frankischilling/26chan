# Dependency and update inventory

JPEG input uses [zune-jpeg 0.5.15](https://docs.rs/zune-jpeg/0.5.15/zune_jpeg/)
and locked zune-core 0.5.3 exclusively in the disposable guest. The direct
declaration disables default features and enables only `std`; x86/NEON SIMD
features are disabled. The inspected upstream source forbids unsafe code in
that configuration. Runtime decoder options also disable unsafe paths, bound
dimensions and progressive scans, and request RGB output. None of this removes
the need for the VM boundary or proves the absence of decoder bugs.

Test-only [jpeg-encoder 0.7.1](https://docs.rs/jpeg-encoder/0.7.1/jpeg_encoder/)
generates reproducible synthetic fixtures. Its default features are disabled
and `std` is enabled; it does not enter shipped normal/build dependency graphs.
Registry metadata and downloaded upstream source were inspected on September
12, 2026. The three new locked registry packages have MSRVs compatible with Rust
1.94. No existing registry version changed. Cargo audit checked 345 dependencies
against 1,243 advisories without findings. The executable dependency check in
`scripts/check-media-parser-dependencies.py` traverses normal/build edges with all
workspace features, rejects guest/JPEG parser paths from the web, media and
observer runtimes, and checks a guest positive control plus an injected-edge
negative control. Rebuild and requalify the guest for decoder updates; see
[JPEG verification](jpeg-media.md).

The public attachment workflow enables [Axum 0.8.9 multipart](https://docs.rs/axum/0.8.9/axum/extract/multipart/struct.Multipart.html)
and reuses locked [Hyper 1.11.1 HTTP/1 client connections](https://docs.rs/hyper/1.11.1/hyper/client/conn/http1/struct.Builder.html)
with Hyper-util 0.1.20's Tokio adapter. The client connects to one validated
numeric loopback socket; it has no DNS, proxy, redirect or URL-fetch facility.
Multipart parsing adds multer 3.1.0 and encoding_rs 0.8.40. The remaining new
registry entries are core_detect 1.0.0, multiversion 0.8.0,
multiversion-macros 0.8.0, multiversion_no_op 1.0.0, simdutf8 0.1.5,
target-features 0.1.6, try-lock 0.2.5 and want 0.3.1. Existing registry versions
are unchanged. The test-only media publisher/reader dependencies do not enter
the public runtime dependency graph.

The multipart crate forbids unsafe code, but its encoding_rs dependency contains
unsafe byte/string and SIMD operations. Its CPU detection and multiversion
dependencies belong to this review surface too. Public upload code reads raw
field chunks, not charset-decoded text or media metadata; this does not establish
that every transitive unsafe path is unreachable or sound. Hyper/Tokio socket
and buffer implementations remain in the public trust base. No native decoder
was added to the public process. Local cargo-audit 0.22.2 scanned 342 locked
dependencies against 1,243 fetched advisories on September 12 without findings.
That result covers known advisories, not a complete dependency security review.

Authenticated HTTP intake adds pinned [Tokio-util 0.7.19](https://docs.rs/tokio-util/0.7.19/tokio_util/io/struct.StreamReader.html)
with its `io` feature to adapt body frames to the existing bounded quarantine
writer. The direct futures-util 0.3.34 declaration reuses its locked version;
default features add futures-macro 0.3.34. Other registry versions and checksums
are unchanged. Intake depends on the existing media crate for bounded storage;
it does not invoke a decoder. Its owned native qualification uses Python's
standard-library client; the separate public caller uses the Hyper client above.

The [maintenance observer](maintenance-observability.md) reuses locked Rustix,
Tokio, Serde and board-observe; only its local package is added to Cargo.lock.
The operator recorder uses Python 3.12+ standard-library descriptor I/O, process
control and synchronization. Those native facilities, systemd and the configured
update commands belong to the maintenance trust base. The observer adds no
database or outbound update client. Native and deployed evidence is tracked in
its [verification record](verification-maintenance-observability.md).

The [resource observer](resource-observability.md) reuses locked Rustix 1.1.4
filesystem calls, Tokio, Serde and board-observe. Its lockfile adds only the local
`board-resource-monitor` package; no registry versions or checksums change.
Linux filesystem/cgroup behavior, systemd and the existing native monitoring/PKI
tools remain part of its trust base. It adds no database client.
Its [September 10 hosted advisory run](https://github.com/frankischilling/26chan/actions/runs/34500076184)
scanned 328 locked dependencies against 1,243 fetched advisories without findings;
npm audit also reported none. This does not establish complete transitive review.

The [authenticated monitoring renderer](authenticated-monitoring.md) adds
operator/test-only pyca bcrypt 5.0.0, installed into an ignored virtual environment
with official binary wheel hashes in `scripts/monitoring/auth-requirements.txt`.
It hashes 64-byte generated Basic passwords at cost 12; applications do not import
it. Synthetic TLS tests use the host OpenSSL CLI and Python's verified TLS stack.
The production profile uses operator-provided PKI, and no Rust registry dependency
changes. Track the [upstream bcrypt release](https://pypi.org/project/bcrypt/5.0.0/)
alongside the pinned native monitoring binaries.

The queue observer reuses locked SQLx/Tokio and board-observe dependencies. Its
lockfile adds only the local `board-monitor` workspace package; no registry
version or checksum changes. Tokio's test-util feature supports deterministic
sampler timing tests. See [queue operations](queue-observability.md) and its
[verification record](verification-queue-observability.md).

HTTP telemetry reuses locked Axum/Tokio/HTTP dependencies and subtle 2.6.1 for
equal-length bearer-token comparison. The fixed metrics crate adds no global
registry or database client. The owned alert qualification downloads official
Prometheus 3.14.0 and Alertmanager 0.34.0 x86_64 Windows/Linux binaries with fixed
SHA256 checks in `scripts/monitoring/download.py`; see
[monitoring operations](http-observability.md). These operator/test tools have a
separate release/update obligation and are not bundled into application binaries.

The approved-media HTTP reader reuses the locked Axum, Tokio, SQLx, PNG/hash and
HTTP-body dependencies. Its lockfile change adds only the local workspace
package; no registry version or checksum changes. Cargo-audit scanned 325
dependencies against 1,243 fetched advisories without a finding on September 9,
2026. The reader never invokes the decoder. Its bounded blocking-file work uses
the locked Tokio 1.53.1 runtime; cancellation and OS shutdown limits are recorded
in [reader verification](verification-media-http.md).

Media dispatch adds pinned [Tokio-Rustls 0.26.5](https://docs.rs/tokio-rustls/0.26.5/tokio_rustls/) and test-only [Rcgen 0.14.10](https://docs.rs/rcgen/0.14.10/rcgen/), checked through registry metadata and downloaded upstream source on September 9, 2026. Their MSRVs are 1.71 and 1.88; both fit Rust 1.94. The client/server configurations explicitly use TLS 1.3 and the locked Rustls/ring versions. Rustix 1.1.4 supplies safe Unix file/identity calls. The added normal dependency tree has no database, decoder, quarantine or publisher crate. Eleven registry packages were added to the lockfile, including test certificate dependencies and optional metadata dependencies; no existing registry version changed. Cargo-audit 0.22.2 scanned 324 dependencies against 1,243 fetched advisories without a finding on September 9, 2026. This covers known advisories at that fetched state, not a complete transitive audit.

Exact Rust dependencies are pinned by `Cargo.lock`; core direct choices are Rust 1.94.0, Axum 0.8.9, Askama 0.16.1 and SQLx 0.9.0. SQLx 0.9 requires Rust 1.94. Official documentation and crate metadata were checked September 8, 2026: [Axum](https://docs.rs/axum/0.8.9/axum/), [Askama](https://docs.rs/askama/0.16.1/askama/), [SQLx](https://docs.rs/sqlx/0.9.0/sqlx/). PostgreSQL 16.15 remains in a [supported major release](https://www.postgresql.org/docs/16/backup-dump.html).

| Component | Pinned/tested version | Role and maintenance note |
|---|---|---|
| Tokio | 1.53.1 in lockfile | Async runtime; native OS integration and unsafe internals are in the trust base |
| Bytes / HTTP body | 1.12.1 / 1.1.0 | Direct declarations reuse existing locked versions. The local `board-http` crate retains admission through response data ownership; Bytes reference counting and its internal unsafe implementation remain in the trust base |
| Rustls / ring | 0.23.44 / 0.17.14 | Database and media-dispatch TLS; ring includes native/assembly code. Dispatch explicitly selects TLS 1.3, trusted roots and required client certificates |
| Tokio-Rustls | 0.26.5 | Media-dispatch async TLS, exact direct pin with default features disabled and ring enabled; no public roots, early data or session resumption |
| Rcgen | 0.14.10 | Test-only synthetic certificates, exact pin with default features disabled and crypto/ring/PEM enabled; fresh harmless keys per test, valid 2020 through 2040, expired control ends 2021 |
| SQLx PostgreSQL driver | 0.9.0 | Bound SQL and PostgreSQL protocol. MySQL/SQLite packages can appear in the lock graph through macro metadata; no SQLite/MySQL driver is enabled in the public normal dependency tree |
| Askama | 0.16.1 | Compiled templates and automatic escaping; application never uses the `safe` filter |
| Argon2 | 0.5.3 | Local deletion passwords, default Argon2id parameters and random salts; four concurrent operations maximum |
| MD5 | md-5 0.11.0, default features disabled | Legacy API checksum of the normalized PNG, not uploaded input. Reuses the version already locked through SQLx; adds no registry package. MD5 is cryptographically broken and is never used for integrity, authorization, IDs or deduplication. Approval and reader verification retain SHA-256. [Upstream documentation](https://docs.rs/md-5/0.11.0/md5/) checked September 12, 2026 |
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
