# Authenticated Media Dispatch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the persisted media queue to isolated processing and durable publication through authenticated, bounded service interfaces.

**Architecture:** A coordinator retains queue/quarantine/publication authority. A mutually authenticated Rust TLS gateway has only local dispatch authority. A root Unix-socket broker authenticates the gateway UID and invokes the existing fixed Firecracker profile with generated paths.

**Tech Stack:** Rust 1.94.0, Tokio, existing Rustls 0.23.44/ring, pinned Tokio-Rustls, Python 3.12, PostgreSQL 16, existing Firecracker 1.16.1.

**Spec:** `docs/superpowers/specs/2026-09-09-media-dispatch-design.md`

## Global Constraints

- Public uploads and production execution remain disabled. No merge, release or production deployment.
- First-party Rust forbids unsafe code. Complex input decoding remains in the guest.
- Request: `IBJOB001` + big-endian u64 length 1..=8,388,608 + exact bytes + write-side EOF. Response: `IBOUT001` + big-endian u64 length 4,194,816 + exact disk + write-side EOF.
- Callers supply no paths, commands, lease tokens, job IDs, filenames or arbitrary metadata to the processing tier.
- Only the coordinator holds database/quarantine/publication authority; only the gateway holds the server TLS key; only the local broker holds host-launch authority.
- Linux peer UID and mutual TLS with fresh bounded leaf authorization are mandatory. No insecure fallback, early data or session resumption.
- Maintain existing strict process cleanup, lease fencing, output validation and approved-only reads.
- Use the current checkout, branch `feature/media-dispatch`, and Git Human Workflow for every Git/GitHub operation. Preserve Francis Hagan's configured identity. Never read `4chan-old` or expose `.local` credentials.
- Verify commands' native exit codes immediately in PowerShell. Use only owned fixtures; do not alter shared database credentials or global Git configuration.

### Task 1: UID-authenticated bounded local broker

**Files:** Create `scripts/media/dispatch-broker.py`, `scripts/media/dispatch_protocol.py`, `tests/media/test_dispatch_broker.py`; modify existing runner only if a narrow import/recovery interface is required, preserving direct CLI behavior. Create `docs/media-dispatch.md` with broker use and its present verification scope.

**Interfaces:** Consumes existing `run-job.py` functions `configuration(path)`, `run(config, source, destination)`, `cancel`, and `job_lifecycle.locked_jobs/reconcile_jobs`. Produces the fixed Unix wire protocol and executable `python3 scripts/media/dispatch-broker.py CONFIG SOCKET_DIRECTORY GATEWAY_UID`; its socket pathname is `SOCKET_DIRECTORY/broker.sock`. Python helpers may be factored within these named files. No network listener, configurable command/backend or credential-bearing environment is accepted.

- [x] Write failing bounded framing and real Unix-socket tests before implementation. Test bad magic; lengths 0, 8,388,609 and u64::MAX; short header/body; appended bytes; missing EOF; fragmented valid input; invalid/extra output; cumulative receive deadline. The core frame examples are:

```python
request = b'IBJOB001' + len(payload).to_bytes(8, 'big') + payload
response = b'IBOUT001' + (4_194_816).to_bytes(8, 'big') + stopped_disk
# Client must shutdown(SHUT_WR); bad frames must never call the executor.
```

Use socketpair/owned pathname sockets and a controlled executor function for protocol-only tests. Verify actual kernel peer credentials with an allowed nonroot nologin identity and a distinct denied identity; denied calls must allocate no request directory and invoke no runner. Install no accounts in library tests; root integration may reuse the provisioned VMM and existing nobody identity only as harmless callers, with the broker configuration refusing the VMM identity for real service use. For a real allowed gateway identity, the owned test harness may create and verify a dedicated nologin `board-media-gateway` account using the existing provisioner's conventions.

- [x] Run `wsl -d Ubuntu -- python3 -m unittest discover -s /mnt/c/Users/imike/4chan-rewrite/tests/media -p test_dispatch_broker.py`; record the expected missing-interface failure. Root-only integration must be explicit and must fail missing prerequisites when requested, not silently skip.
- [x] Implement streaming protocol validation and the broker. Check `SO_PEERCRED` before receiving input or allocating staging. Use exact generated root-owned paths, exclusive broker lock, atomic/private allocation, absolute three-second receive/send deadlines, and the existing fixed runner. Require request EOF before execution. Return bounded regular stopped output only after runner cleanup. Handle SIGINT/SIGTERM through the existing runner cancellation handler and reset handlers per request. Reject APP_ENV other than development and credential-bearing environment names. Static errors only.

```python
pid, uid, gid = struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i')))
if uid != allowed_uid:
    raise PermissionError('dispatch caller rejected')
# Only after this check may receive_request create its generated input file.
```

Startup must reconcile existing VMMs under their own lock before deleting verified abandoned request directories. Accept only root-owned private `request-<32 hex>` directories and expected regular `input`/`output` files; reject symlinks, unexpected entries/mounts/ownership and retain them. Verify stale socket type/owner under the broker lock before replacing it. Preserve lock files. A second broker must fail without changing the first's state.

- [x] Add meaningful root integration for one real PNG decode over the allowed UID, denied UID, malformed request with no VM start, cancellation while the VMM is live, stale request recovery and unknown-entry refusal. Use the existing probe/decoder configs and `VmTest.assert_clean`. Retained unknown fixtures must be explicitly cleaned by their owning test after assertions; never broad-scan or kill unrelated processes.
- [x] Run the focused nonroot tests, explicit root broker tests and relevant existing VM/recovery suite after any runner change. Compile Python. Record red/green commands and results in the report, document service lifetime/deadline limitations, self-review, and commit only this task's files through the Git wrapper.

### Task 2: Bounded mutual-TLS transport and unprivileged gateway

**Files:** Create `crates/media-dispatch/{Cargo.toml,src/lib.rs,src/protocol.rs,src/tls.rs,src/gateway.rs,src/config.rs,src/bin/media-dispatch-gateway.rs,tests/protocol.rs,tests/tls.rs}`; modify root `Cargo.toml`, `Cargo.lock`, dependency inventory and `docs/media-dispatch.md`. Files may be reduced if unnecessary, not expanded into unrelated frameworks.

**Interfaces:** Produces `ClientSettings::read(path: &Path) -> Result<ClientSettings>`, `DispatchClient::new(settings: &ClientSettings) -> Result<DispatchClient>`, `DispatchClient::process<R: AsyncRead + Unpin>(&self, input: R, length: u64) -> Result<Vec<u8>>`. Produces a Linux gateway CLI `media-dispatch-gateway GATEWAY_CONFIG` with validated operator JSON containing listen address, server cert/key, client CA, authorization file and broker socket path. Library error type exposes static categories, not secret values. Consumes Task 1's exact fixed protocol and root peer requirement.

- [x] Add the crate and test scaffold; pin the maintained Tokio-Rustls version after checking primary docs/registry, reuse existing locked Rustls 0.23.44 with ring and TLS 1.3 only. For synthetic certificate generation choose a maintained pinned test-only generator or generate public harmless fixtures; document provenance and validity. Add failing AsyncRead framing/property tests and real loopback TLS authentication tests. Example invariants:

```rust
assert!(read_request(&mut stream_with_length(8_388_609)).await.is_err());
assert!(client_with_wrong_server_name.process(input, size).await.is_err());
assert_eq!(authorized_backend_calls.load(Ordering::SeqCst), 0);
```

Test valid mutual authentication; absent, wrong-CA, expired and removed client identity; malformed/missing authorization file; server-name/CA mismatch; request/response truncation and extra bytes; slow handshake/intake; overload before input allocation. Verify revocation after handshake and before forwarding/response, not merely startup configuration rejection. The valid control must reach the owned backend in the same fixture.

- [x] Implement exact bounded protocol helpers, absolute deadlines and private bounded config/key loading. Use explicit trusted roots, standard certificate validation and client authentication, TLS1.3 only, no early data or resumption. Authorize the validated leaf SHA-256 using a fresh file read after input and before response. Reject more than eight fingerprints, duplicates, malformed lines, untrusted writable permissions and unavailable files. No public roots, proxies or permissive verifier.

```rust
let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(roots.into(), provider.clone()).build()?;
let mut server = rustls::ServerConfig::builder_with_provider(provider)
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_client_cert_verifier(verifier)
    .with_single_cert(chain, key)?;
server.send_tls13_tickets = 0;
server.session_storage = std::sync::Arc::new(rustls::server::NoServerSessionStorage {});
```

The gateway must be nonroot, reject unrelated credentials, bound simultaneous handshakes to four and authorized work to one with immediate overload rejection. Authenticate/validate the request before connecting to the fixed Unix broker; verify broker peer UID 0. Enforce a three-second handshake and intake, twenty-five-second processing exchange, exact response framing and close-notify EOF. Cancellation drops only owned connections; existing broker/VM deadlines remain independent. The client transmits only bounded bytes, rejects changed input/trailing response and times out without returning partial output.

- [x] Run focused protocol/TLS tests red/green, crate Clippy with warnings denied, formatting, relevant broker conformance checks, and dependency audit for added pins. Record actual commands/results, self-review, and commit the task through the wrapper.

### Task 3: Fenced coordinator integration and complete qualification

**Files:** Modify `apps/media-admin/{Cargo.toml,src/bin/media-publish.rs,src/lib.rs,tests/cli.rs,tests/approval.rs}`, `crates/media/src/quarantine.rs` and its tests if an exact-ID input-opening API is needed; create `scripts/test-media-dispatch.sh` and candidate gateway/broker unit/config examples under `deploy/`; modify `.github/workflows/ci.yml`, `docs/{media-dispatch.md,architecture.md,media-approval.md,firecracker.md,readiness.md}` and add `docs/verification-media-dispatch.md`.

**Interfaces:** Consumes Task 2's client API and existing `MediaQueue::claim/fail`, `ValidatedOutput::read_disk`, and `board_media_admin::publish`. Produces `media-publish dispatch CLIENT_CONFIG PRIVATE_STORE`, printing only an approved ID. New quarantine input access must accept `ObjectId`, verify regular bounded input and exact stored byte count, and expose no caller-selected path.

- [ ] Write failing CLI/database tests showing the missing dispatch command, failed transport/invalid output grants no approval, a stale/expired lease cannot publish a delayed valid response, and valid input produces an approved-only reader result. Require configuration validation before queue claim. The essential fence stays:

```rust
let job = queue.claim().await?.ok_or("no queued job available")?;
let token = job.lease_token.as_deref().ok_or("missing lease")?;
// Only exact generated input bytes cross DispatchClient::process.
let output = ValidatedOutput::read_disk(std::io::Cursor::new(disk)).await?;
let asset = publish(&queue, &store, &job.id, token, &output).await?;
```

- [ ] Implement the command with existing explicit development/credential checks, bounded client processing and validation, fenced failure recording and existing publication locks. Initialize config/quarantine/store before claim, never send job IDs/tokens/filenames, and never retry automatically or publish raw/partial output. Use static errors and keep approval checks authoritative after timeouts or uncertain commits.
- [ ] Implement the owned native Linux harness: private synthetic PKI, nologin gateway UID, root broker, actual Rust TLS gateway/client, disposable queue intake, actual Firecracker decode, durable approval and restricted read. Verify unauthorized requests leave no VM/staging/approval, actual service identities cannot read each other's privileged credentials/stores, cancellation leaves no active VMM/reusable workspace, and all owned resources stop on cleanup. Preserve shared database credentials. Include lease-expiry/failure tests using owned queue records and deterministic synchronization rather than long arbitrary sleeps.
- [ ] Add CI execution after existing VM qualification; run current existing suites plus the real dispatch harness. Add reviewed candidate units documenting exclusive socket directory, root-only staging, gateway key/authorization permissions, fixed config, resource bounds and controlled shutdown. They must reject production mode until the recorded deployed prerequisites are satisfied. Update architecture's stale publication state accurately, existing media docs and readiness without claiming parity or production approval.
- [ ] Run native/local relevant Rust/DB/TLS/broker/VM tests, full required formatting/Clippy/browser/migration/restore checks and current-head hosted Linux/Windows/advisory workflows. Record exact versions, commands, red failures, final results and unrun deployment tests. Commit through the wrapper, obtain task and broad source reviews, push the authorized branch and open a draft PR stacked on #21 linked to #5. Update #5 only with verified results; do not merge or enable uploads.

## Completion check

All three tasks must have real behavior, passing relevant tests, source-review disposition and committed verification evidence. The final source review covers the whole base-to-head diff and cross-task authority, deadline, protocol and lease relationships. Current-head CI must pass before presenting this dispatch slice as complete. The full rewrite goal stays active because production/reference requirements remain outstanding.
