# Authenticated media dispatch

The development coordinator claims one queued input, sends its exact bytes through the unprivileged mutual-TLS gateway and UID-authenticated root broker, validates the stopped Firecracker disk, and publishes under the existing lease fence. The owned native harness exercises this entire path and restricted reads. Deployed qualification and public uploads remain blocked; see [verification evidence](verification-media-dispatch.md).

## Run the local broker

Prepare the pinned decoder artifacts and VMM identity using [the Firecracker runbook](firecracker.md). On the owned host, provision a separate system account named `board-media-gateway` with no home and `/usr/sbin/nologin`, following the existing provisioner's account conventions. Verify the account before reusing it. Its UID and primary GID must be nonzero, and its UID must differ from `board-media-vmm`.

Run from trusted, operator-owned code with an absolute root-owned artifact configuration and an absolute socket directory whose parent already exists:

```sh
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin APP_ENV=development \
  python3 scripts/media/dispatch-broker.py \
  /tmp/26chan-media-test/decode.json /run/26chan-media-dispatch GATEWAY_UID
```

Replace `GATEWAY_UID` with the verified decimal UID. Startup accepts only `APP_ENV`, `PATH`, `LANG`, `LC_ALL`, `LC_CTYPE` and `TZ` in the environment; every other inherited variable is rejected. `APP_ENV` must equal `development`. Keep gateway TLS keys and coordinator credentials in their separate processes.

The broker creates a root-owned socket directory with mode 0750 and the gateway's primary group. An existing directory must have those exact settings. The socket is `broker.sock`, root-owned, mode 0660, with that group. `SO_PEERCRED` must report the configured UID before intake or request allocation; membership in the socket group grants no execution authority. The CLI has no command, backend or network-listener option.

## Stream protocol

Each connection carries one request and one response. Integers are unsigned 64-bit big-endian values.

| Direction | Magic | Payload bytes | Terminator |
| --- | --- | --- | --- |
| Request | `IBJOB001` | 1 through 8,388,608 | Write-side EOF |
| Response | `IBOUT001` | Exactly 4,194,816 | Write-side EOF |

The sender writes the eight-byte magic, eight-byte length and exact payload, then calls `shutdown(SHUT_WR)` while keeping its read side open. Bad magic, invalid lengths, truncation, trailing bytes and missing EOF reject the request before execution. Input and output use fixed scratch buffers. Each receive/send transfer has its own absolute three-second deadline, including framing and EOF; progress does not extend it. A rejected request closes the stream. Runtime messages use static categories and include no input bytes, private paths or exception values.

Only after the runner returns from VM cleanup does the broker send the exact regular output file. The disk remains untrusted: a receiver must check framing and use the existing full stopped-disk validator before approving media.

## Lifetime and recovery

One broker holds a permanent `broker.lock` and serves one request at a time. A second broker fails before changing the active socket or request collection. Request files live in root-only `requests/request-<32 lowercase hex>/` directories. The only accepted entries are regular root-owned, mode-0600, single-link `input` and `output` files; request directories are mode 0700. Allocation is exclusive and private. The broker does not mount a guest filesystem.

SIGTERM and SIGINT use the runner's cancellation handler. The broker resets that handler for each request and after runner cleanup because the runner suppresses repeated signals while unwinding. Cancellation during a job stops its complete service before removing request storage. If cleanup cannot establish a stopped state, the broker retains the request and exits.

A disconnected caller may consume the slot until the existing runner deadlines expire. Those include a 15-second service deadline, two-second stop grace and an independent 30-second launch-client monitor with two-second kill grace. Recovery and host commands have their own bounds; the broker does not promise a single three-second or 25-second job lifetime. It has no proactive disconnect cancellation while the runner is executing and no overall daemon lifetime limit.

After SIGKILL, startup acquires the broker lock, then the existing runner lock and reconciles managed VM services/workspaces before deleting abandoned requests. A surviving launch client holds the runner lock until it exits, so an immediate restart may fail and must be retried by the operator. Recovery validates the entire request inventory before deletion and refuses more than sixteen abandoned requests. Unknown entries, symlinks, hard links, ownership/permission mismatches and mounts retain storage and block startup. Inspect the exact retained paths; do not broadly delete the request or VM collections. Lock files remain in place. A stale socket is replaced only under the broker lock after verifying its exact pathname, root ownership and socket type.

## Local verification

The default suite runs framing tests without root and skips root integration unless explicitly requested:

```sh
python3 -m unittest discover -s tests/media -p test_dispatch_broker.py
```

Explicit integration requires the dedicated gateway account, the provisioned VMM account, the existing `nobody` account, reviewed decoder/probe configurations and the owned Linux VM prerequisites. Missing prerequisites fail the requested run:

```sh
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_DISPATCH_ROOT_TESTS=1 \
  MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-test/decode.json \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-test/probe.json \
  python3 tests/media/test_dispatch_broker.py -v
```

Local WSL evidence covers an actual harmless PNG decode through Firecracker, allowed/denied kernel peer UIDs, malformed framing without executor calls, absolute deadlines, exclusive broker ownership, live-VM SIGINT/SIGTERM cleanup, SIGKILL recovery behind the inherited launch lock, and refusal of unknown request storage. Tests use owned pathname sockets, temporary request storage and the existing `VmTest.assert_clean` checks. The owning tests explicitly remove their retained unknown fixtures.

These broker tests qualify the local launcher. The complete coordinator harness below additionally covers queue lease/publication fencing. Neither qualifies public uploads, a dedicated processing host, deployed service operation, power-loss recovery or a separate media origin. The broader remaining requirements are recorded in the [dispatch design](superpowers/specs/2026-09-09-media-dispatch-design.md).

## Configure mutual TLS

Build `media-dispatch-gateway` from the `board-media-dispatch` crate for Linux. Run it as the separate `board-media-gateway` account with a cleared environment, after starting the root broker:

```sh
sudo -u board-media-gateway env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  APP_ENV=development media-dispatch-gateway /etc/26chan-dispatch/gateway.json
```

The CLI rejects root UIDs/GIDs, unequal real/effective identities, every environment variable outside the broker's documented allowlist, and any `APP_ENV` other than `development`. It takes exactly one configuration path. It has no database, decoder, quarantine or publisher dependency. It connects only to the configured broker pathname and checks that the kernel reports broker peer UID 0 before sending any request bytes.

The gateway JSON has these exact fields; unknown fields are rejected:

```json
{
  "listen": "127.0.0.1:9443",
  "server_certificate": "/etc/26chan-dispatch/server.pem",
  "server_key": "/etc/26chan-dispatch/server.key",
  "client_ca": "/etc/26chan-dispatch/client-ca.pem",
  "authorization_file": "/etc/26chan-dispatch/authorized-clients",
  "broker_socket": "/run/26chan-media-dispatch/broker.sock"
}
```

The coordinator's client JSON is separate:

```json
{
  "endpoint": "127.0.0.1:9443",
  "server_name": "dispatch.example.internal",
  "server_ca": "/etc/26chan-coordinator/server-ca.pem",
  "client_certificate": "/etc/26chan-coordinator/client.pem",
  "client_key": "/etc/26chan-coordinator/client.key"
}
```

Socket addresses are literal IPv4/IPv6 addresses; the client endpoint port must be nonzero. The DNS server name is verified against the server certificate and cannot be an IP address or URL. No name lookup, proxy discovery or public certificate roots are used. CA and certificate files contain PEM certificates; certificate chains are leaf first. Private keys are PEM PKCS#1, PKCS#8 or SEC1 supported by Rustls. Issue separate service identities with the appropriate server-auth or client-auth usage. The library exposes `ClientSettings::read(&Path)`, `DispatchClient::new(&ClientSettings)` and `DispatchClient::process(input, length)` for an `AsyncRead + Unpin` input.

Configuration and TLS files must be nonempty regular files of at most 65,536 bytes, with absolute paths and no `..` component. On Unix they must have one link, be owned by root or the process's effective UID, and have no group/world write permission. Keys must additionally have no group/world permissions; use mode 0600 or 0400 owned by the service that reads them. Final-component symlinks and special files are rejected. Keep all parent directories operator protected on a trusted local filesystem. On Windows the development client checks file type, size and paths; the operator must provide equivalent private ACLs and protected parents. Windows is not a gateway platform.

The authorization file must be root-owned, readable by the gateway, single-link, and not group/world writable; mode 0644 in a root-owned protected directory is suitable. Its limit is 520 bytes. Each line is one lowercase 64-hex SHA-256 fingerprint of a client's DER leaf certificate. It accepts one through eight distinct entries and an optional final newline. Empty entries, CRLF, uppercase, whitespace, duplicates, extra entries, missing files and unreadable files all reject authorization. An empty list denies service by failing validation. This file grants no authority to a certificate that fails ordinary chain, time or client-auth validation.

Replace the authorization file atomically as root, retaining ownership and permissions. The gateway reads it at startup, after each TLS handshake, after complete intake and immediately before response transmission. Revocation observed by the post-intake check prevents broker contact; revocation observed by the final check prevents response transmission. Revocation during a response already being transmitted cannot retract bytes already sent. For certificate/key/CA rotation, provision new per-service files, overlap authorized fingerprints as needed within the eight-entry limit, restart the affected service/client, test the new identity, then remove the old fingerprint. No generated test key is suitable for deployment.

## Transport bounds and tests

Both ends use Rustls 0.23.44 and ring with TLS 1.3 only, explicit trust roots, mandatory client authentication, no early data and no resumption. The client sends TLS close-notify after the request while keeping its read side open; the gateway does the same after the response. A TCP EOF without close-notify, extra bytes or truncated framing rejects the transfer. Transport success returns the fixed disk bytes, never approval.

The gateway allows four concurrent handshakes and one authorized request, with immediate excess-connection rejection and no application request queue. It acquires the work permit before allocating input. Handshake and full request intake each have an absolute three-second deadline. The broker exchange and response transmission together have an absolute 25-second deadline. The client bounds connect plus handshake to three seconds, request streaming to three seconds and response intake to 25 seconds. Progress does not reset these deadlines. The client checks the source has exactly the declared length and returns no partial output. Framing uses 16 KiB scratch buffers, a bounded request allocation and one fixed-size output allocation.

Dropping a gateway serve future aborts its owned connection tasks. Timeouts and client cancellation drop owned connections; they do not signal or stop the root broker's runner. An already-started job can continue under its independent broker/VM deadlines, and the coordinator must still apply its current lease fence before publication. Runtime failures use static categories without exception values, credentials, input or private paths.

Portable framing/property and owned loopback TLS tests run with:

```sh
cargo test -p board-media-dispatch --test protocol --test tls
cargo clippy -p board-media-dispatch --all-targets -- -D warnings
cargo fmt --all -- --check
cargo audit
```

The Linux gateway tests execute only when explicitly enabled. They require root, the existing `nobody` UID/GID 65534, `/usr/sbin/runuser`, `/usr/bin/setpriv` and Python 3. They use temporary local sockets and harmless fixed byte arrays, never a decoder. An explicit request fails if these prerequisites are unavailable:

```sh
sudo env MEDIA_DISPATCH_ROOT_TESTS=1 cargo test -p board-media-dispatch --test tls
```

Those cases cover a real nonroot CLI reaching a root peer, denial of a nonroot broker before any request bytes, a valid same-fixture control, post-admission and post-processing revocation, unavailable/malformed/writable authorization, malformed broker responses, four-handshake/one-work admission, absolute deadlines and cancellation. The portable TLS cases cover valid mutual authentication; absent, wrong-CA, expired and wrong-usage client certificates; server-name/CA mismatch; changed input; malformed responses; close-notify and deadlines. Root-only cases are omitted on Windows and when the environment flag is absent; a normal portable pass does not establish Unix peer or gateway identity enforcement.

Rcgen 0.14.10 generates fresh synthetic keys for each test. Valid leaves/CAs use January 1, 2020 through January 1, 2040; the expired leaf ends January 1, 2021. Fixtures contain no production identities or retained private keys. The owned TLS fixture confirms that request close-notify leaves response reads functional with Tokio-Rustls 0.26.5.

## Fenced coordinator

Build `cargo build -p board-media-admin -p board-media-dispatch --bins --locked`. In a fresh shell with only the existing media writer credential, run:

```sh
source .local/media.env
export APP_ENV=development
export MEDIA_QUARANTINE_DIR="$PWD/.local/quarantine"
target/debug/media-publish dispatch /etc/26chan-coordinator/client.json "$PWD/.local/objects"
```

The command initializes TLS configuration, quarantine and publication storage before claiming exactly one job. Use the one canonical private output root associated with the database, as required by [publication recovery](media-approval.md). It opens only the generated `ObjectId` input and checks regular-file type, the 1..8,388,608-byte bound and exact recorded size. Protected parent directories are mandatory; metadata checks do not protect a directory writable by hostile local processes. The transport rechecks exact length and EOF. Only input bytes cross the boundary; display filenames, queue IDs, lease tokens and database credentials remain with the coordinator.

Processing has an outer 29-second timeout in addition to the transport's phase deadlines. The queue lease remains 30 seconds and is checked by publication after full disk validation; reaching a transport deadline never extends it. Current failures record `processing_failed` or `invalid_output` using the same token and unexpired-lease predicate. No automatic retry occurs. A stale failure cannot mutate a replacement lease. Database failures and uncertain approval commits print no output; inspect authoritative approval state and reconcile using the existing lock before an operator retry. Publication errors preserve possibly approved files. Successful stdout contains only the approved opaque asset ID.

## Complete owned native qualification

The harness requires root, working reviewed decoder/probe configurations, OpenSSL, PostgreSQL 16 client tools, the existing disposable cluster and credentials, and compiled native binaries. It validates an idle disposable queue and reuses credentials without provisioning or rotation. It creates or verifies distinct `board-media-coordinator` and `board-media-gateway` nologin accounts, copies binaries to root-owned generated `/run` storage, generates private two-day test PKI, and stops its owned processes and removes only its generated files and queue records on cleanup. Do not run it alongside another queue consumer or VM qualification.

```sh
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-test/decode.json \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-test/probe.json \
  MEDIA_DISPATCH_BIN_DIR="$PWD/target/debug" \
  bash scripts/test-media-dispatch.sh
```

Replace example artifact paths with the actual reviewed files. `MEDIA_DISPATCH_BIN_DIR` defaults to `target/debug`. Missing required fixtures fail; this harness has no skip or stub path. Actual Rust client/gateway, root broker, Firecracker decoder, queue, validator, publication and restricted DB reader all participate. It checks approved-only reads before/after queue cleanup, identity file denials with healthy allowed readers, revoked authorization without staging/VM/approval, real decoder refusal, and lease expiry/replacement synchronized by stopping and resuming the owned broker. A separate probe run cancels a live VMM and checks complete cleanup. The probe is never substituted for the decoder success case.

The reader command in this harness uses the coordinator filesystem UID with only the restricted reader database credential. This qualifies approved-view authority and command behavior, not a separate deployed reader filesystem sandbox. Root remains trusted host administration and can access host files; gateway, coordinator and VMM identities are the filesystem denial subjects. Separate TLS/database tests use synchronized controlled responses to isolate invalid output and stale leases; those tests are not full-path VM evidence.

## Candidate services

The `deploy/media-dispatch-{broker,gateway}.service` and JSON/environment examples are reviewed development candidates. Install trusted scripts under `/opt/paperboard/media`, the gateway binary at `/opt/paperboard/media-dispatch-gateway`, and configurations in protected `/etc/26chan-*` directories matching the units. Resolve and verify the gateway account's numeric UID before replacing `GATEWAY_UID` in root-owned mode-0600 `dispatch.env`. Its placeholder is intentionally invalid. Preserve `APP_ENV=development`; both programs reject `production` and `env -i` preserves that selected value while removing systemd-injected variables. No production approval is implied by changing the file.

The socket directory remains root-owned 0750 with the gateway primary group; `requests` remains root-only 0700. Keys are private and service-owned; authorization/configuration are protected root-owned files readable as required. Keep coordinator credentials, quarantine and output storage outside gateway/VMM access. Never grant the gateway write access to scripts, artifacts, socket directory, authorization or configuration. Socket presence and `Type=exec` are insufficient readiness: exercise an approved dispatch and restricted read.

The gateway candidate has no capabilities and restricts filesystem writes, namespaces, devices and address families while retaining Unix and IP sockets. It explicitly makes `/run` read-only; the gateway connects to the broker's existing socket and creates no host runtime files. The broker is trusted root with host mount, KVM, cgroup and system-manager authority; its independently launched VMM has the runner's separate transient-service limits. Candidate supervisor budgets are gateway 128 MiB/32 tasks/one CPU and broker 256 MiB/64 tasks/one CPU, not measured saturation results. Neither unit automatically restarts or enables itself. Stop the gateway before the broker; the gateway binds its lifecycle to the broker. The broker has a 90-second stop grace, after which forced termination still requires inspected recovery. Permanent locks and uncertain request state are not removed by systemd `RuntimeDirectory` cleanup.

Run the existing disposable harness with `--systemd` to load uniquely named temporary copies of these units and execute the real pipeline under their policies. This requires root, a running system manager, native binaries, reviewed VM configurations and an idle disposable database. The [service verification record](verification-dispatch-services.md) covers actual startup, environment clearing, kernel limits, filesystem witnesses, dependency shutdown and inspected retained-state recovery. It also records the writable `/run` finding that required the explicit restriction. These local development checks do not qualify production deployment or saturation.
