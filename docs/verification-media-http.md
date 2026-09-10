# Approved media HTTP verification

On September 9, 2026 (September 10 UTC), the development reader completed local
Rust, browser and actual Linux service qualification. This extends the approved
output path with separate HTTP serving; it does not enable public intake or
qualify production. See the [design](media-http-design.md), [operating
notes](media-http.md) and [remaining prerequisites](readiness.md).

## Local checks

Windows used Rust 1.94.0, the committed lockfile, PostgreSQL 16.15 in the existing
owned WSL cluster, Node 25.2.1 and pinned Playwright 1.62.0/Chromium 151. The
native service used Ubuntu 24.04, WSL kernel 5.15.153.1, systemd 255 and the
previously recorded Firecracker 1.16.1/guest Linux 6.1.186 artifacts.

- `cargo build --workspace --examples --bins --locked` and
  `cargo test --workspace --all-features --locked` passed on Windows. This
  includes real reader-role database/router checks and a browser embedding an
  image from the actual `board-media-http` executable on another loopback port.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  passed. `cargo fmt --all -- --check`, actionlint for both workflows, and
  `git diff --check` passed before publication.
- `npm test` passed all nine public behavior/visual checks; `npm run test:staff`
  passed the synthetic WebAuthn workflow. Existing visual baselines were unchanged.
- `cargo audit` fetched 1,243 advisories and scanned 325 dependencies without a
  finding; `npm audit --audit-level=low` found zero vulnerabilities. An initial
  `cargo audit --locked` invocation rejected the unsupported option; the actual
  scan used `cargo audit`, which reads Cargo.lock.
- The native reader/admin binaries built with the same lockfile. The Unix shared
  publication regression passed, including exact directory/file permissions and
  group inheritance while preserving owner-only default publication.

Configuration tests first failed against missing/rejecting interfaces, the
shared-store test rejected its initial stub, and the real HTTP test failed
against an empty router before implementation. Two deliberate negative controls
also failed: changing CORP to `same-origin` prevented browser image display, and
releasing a blocking permit early broke the cancellation-ownership regression.
Both mutations were restored; the positive tests and full workspace suite then
passed. These checks use harmless synthetic files and owned disposable records.

## Actual service checks

The final rebuilt native binary passed `scripts/test-media-dispatch.sh --http`
with `MEDIA_DISPATCH_BIN_DIR=/opt/26chan-rust/target/debug`. The recorded local
VM profiles were `/tmp/26chan-media-repro-20260909/decode.json` and
`/tmp/26chan-boundary-probe-t1wsmj04/probe.json`. They are local inputs, not
portable deployment configuration.

`scripts/test-media-dispatch.sh --systemd` also passed with the rebuilt
publication binary, covering the original private-store default, real dispatch,
expired/replaced leases, live-VM cancellation and owned-state recovery/cleanup.

The qualifier ran actual candidate broker/gateway/reader units with temporary
owned paths. Queue intake reached approved PNG output through authenticated
dispatch and Firecracker before the separate HTTP reader served those bytes.
It checked the reader process UID/GID/groups, empty capabilities, lack of other
database credentials, and actual memory/CPU/task/file-descriptor limits. Local
cgroups used v1; the fixture also supports v2 for Linux CI.

Healthy same-identity controls established readable output and writable external
witnesses before checking denied output writes and read-only service mounts.
Protected coordinator/private witnesses remained unreadable. A probe entered
the actual service mount namespace and root before dropping to the reader UID;
this checks mounts and identity, not inherited service seccomp filters. The
actual reader database login could read the approval view but received SQLSTATE
42501 for content/staff reads and media updates, with healthy privileged controls.

HTTP tests covered exact PNG bytes, HEAD, cache revalidation, physically readable
pending output denied by approval, corrupt bytes denied even with matching ETag,
and removal of approval. Actual service restarts rejected production mode and
inherited application credentials with exit status 1. Owned units, processes,
files and database fixtures were cleaned. The nonlogin reader setup account is
deliberately retained.

A standalone template verifier initially failed because its `/opt/paperboard`
binary was not installed. The fixture now verifies the rendered temporary unit
against the actual installed test binary before starting it; that verification
and the resulting service checks passed. No template-only check is presented as
deployed enforcement evidence.

## Review and limits

A separate read-only source review examined the tracked and new runtime, tests,
configuration and qualification files. It found no blocking correctness or
security issue. Its minor compatibility-document inconsistency was corrected to
distinguish implemented development serving from production domain deployment.
The reviewer did not independently rerun the service qualification.

The application retains 16 response permits and four blocking-file permits;
the latter survive cancellation until work finishes. This does not bound total
process memory, cancel a stalled filesystem operation, or impose socket-write
deadlines. Production load/saturation, DNS/TLS/domain and network isolation,
storage/power-loss/backup evidence, monitoring and independent deployment review
remain open. Windows browser results do not establish Unix storage durability.
Post attachments, legacy media/thumbnails and original-download policy remain
separate work. Production mode and public media enablement are still rejected.

Hosted final-head CI is recorded on the pull request; local checks alone do not
establish a hosted pass or authorize a production release.
