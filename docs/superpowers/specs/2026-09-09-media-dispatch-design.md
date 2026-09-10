# Authenticated media dispatch

The implementation prompt requires authenticated service requests across the media boundary. The current operator workflow manually copies a claimed job into a root runner and later publishes its stopped output. This design connects those steps without sending queue credentials, lease tokens, quarantine paths or publication authority to the processing tier. It extends the verified `db60ab7` checkpoint and remains disabled for production and public uploads until deployed qualification is complete.

## Architecture and choice

Use a Rust mutual-TLS gateway on the processing host and a separate local root launcher. The coordinator keeps the existing `board_media` login, quarantine access and fenced publication authority. It claims one job, sends only its bounded input bytes, validates the returned stopped disk, and invokes the existing durable publisher. The gateway holds only its server key and authority to request one bounded local decode. The root launcher holds runtime/host authority but has neither TLS keys nor a network listener.

An SSH forced-command transport would add remote-account and command parsing policy to every dispatch. A TLS endpoint inside the root launcher would expose TLS parsing and credentials to host authority. The chosen separation keeps the remote parser unprivileged and the root interface fixed and local. It is one serialized processing slot, consistent with the existing VMM identity and publication profile; it is not a general remote execution API.

## Fixed protocol

Both TLS and local Unix streams carry exactly one request and one response:

- Request: eight bytes `IBJOB001`, an unsigned big-endian u64 length between 1 and 8,388,608 inclusive, that many input bytes, then write-side EOF. Extra bytes, truncation, bad magic or out-of-range length reject the request before execution.
- Response: eight bytes `IBOUT001`, an unsigned big-endian u64 length equal to 4,194,816, exactly that many stopped-disk bytes, then write-side EOF. The coordinator rejects any other framing, truncation or trailing bytes, and still applies `ValidatedOutput::read_disk` before publication.
- There are no command names, paths, lease tokens, arbitrary metadata, archives or worker-selected filenames. A rejected request closes the stream; transport completion never means output approval.
- Input/output transfers use fixed scratch buffers and independently enforced lengths. Socket read/write deadlines are absolute across each transfer, so a slow sender cannot extend them one byte at a time.

TLS 1.3 permits write-side closure while receiving the response. The client sends close-notify after its one request; the server sends close-notify after its one response. Plaintext, TLS early data and session resumption are disabled. The gateway validates the entire request before contacting the privileged launcher.

## Local launcher

`scripts/media/dispatch-broker.py CONFIG SOCKET_DIRECTORY GATEWAY_UID` runs on an owned Linux host as root in explicit development mode with a cleared environment. The configuration is the existing root-owned, hash-checked decoder configuration. The configured gateway UID must be nonzero, have a nonzero primary GID and a nologin account. It cannot be the VMM identity. These values and paths come only from operator configuration.

The broker creates a pathname Unix socket `broker.sock` below a root-owned directory, with mode 0660 and the gateway's primary group. The directory is root-owned, group-searchable and not writable by the gateway. Before reading request bytes or allocating a job workspace, the broker checks Linux `SO_PEERCRED` and accepts only the exact configured UID. Group permission alone never grants execution. Callers cannot pass file descriptors or choose a backend.

One broker holds an exclusive permanent lock in its root-owned directory and handles one request at a time. It stores input and output under generated `request-<32 lowercase hex>` directories below a root-only `requests` directory. Stream intake is bounded to the declared input length; no guest filesystem is mounted. The existing runner creates its own isolated VM workspace, performs stopped-output collection and terminates the complete service before returning. The broker returns that exact bounded regular output file only after the runner has completed cleanup.

The broker uses the runner's existing service/runtime deadlines. Ordinary cancellation unwinds the current runner before removing request files; request receive/send deadlines are three seconds each. Disconnection or coordinator timeout may leave the already-started job running until its existing bounded runner deadline, but never permits a second concurrent VMM or approval. The broker resets cancellation handling before each request because the runner deliberately suppresses repeated signals while unwinding.

After SIGKILL, startup first acquires the existing job lock and reconciles VMM services/workspaces. Only after no managed process remains may it remove known abandoned request files. Request reconciliation accepts only generated directories with the expected root ownership, permissions and fixed regular input/output filenames. Unexpected files, symlinks, mounts or ownership block startup and remain for inspection. The socket may be replaced only after acquiring the broker lock and verifying a stale root-owned socket at the exact configured pathname. No broad filesystem deletion is allowed.

## Mutual TLS and authorization

`board-media-dispatch` is a safe-Rust transport library plus Linux gateway binary. It has no dependency on the database, media decoder, quarantine or publisher. Reuse the locked Rustls 0.23.44 and ring provider; add pinned Tokio-Rustls after checking the maintained upstream version. Use standard certificate verification, never a custom accept-all verifier. Trust files are explicit private-purpose CAs, not operating-system public roots.

The gateway requires a valid client certificate chain with client-auth usage and current validity. It also requires the leaf certificate's SHA-256 fingerprint in a bounded operator-owned authorization file: one lowercase 64-hex fingerprint per line, at most eight entries, no empty/duplicate/invalid entries. Read authorization state afresh after receiving a complete bounded request and before sending a response. Missing, unreadable, malformed or revoked authorization rejects access. Certificate rotation uses an atomic operator replacement; revocation removes the fingerprint. In-flight work may finish, but a removed authorization must not receive a later response. Disable TLS resumption so every new request revalidates certificate time and trust.

The client verifies the configured server name and certificate chain against its explicit server CA. No insecure development TLS switch exists. Endpoint configuration uses a literal IP socket address plus a separate validated DNS server name; no proxy environment variables or caller-supplied URL routes are used. TLS/configuration/key files are bounded regular files. Keys require private permissions on Unix; operators must supply equivalent private Windows ACLs for development client tests. The Linux gateway must run as a nonroot identity and rejects inherited database, deployment and unrelated credentials.

The gateway permits at most four bounded handshakes and one active authorized request, with no application request queue. A handshake has a three-second deadline, request intake three seconds and the complete processing exchange a twenty-five-second deadline. Excess work is rejected before input allocation. It connects only to the configured Unix socket and requires the broker peer UID to be root. It validates both request and response framing. Failure never falls back to local decoding or direct publication. Cancellation drops connections; the root runner's separate deadlines and recovery continue to govern a job already started.

## Coordinator and state

Add `media-publish dispatch CLIENT_CONFIG PRIVATE_STORE`. Validate transport configuration, quarantine and output roots before claiming work. The client configuration is bounded JSON with `deny_unknown_fields`, absolute CA/certificate/key paths, literal socket address and server DNS name; it contains no database credentials. Existing explicit development-mode and credential checks remain in force.

Claim exactly one queued job through the existing thirty-second lease. Open only that generated job ID's quarantine input, require a bounded regular file and exact queued byte count, stream it to the authenticated gateway, and accept only the fixed response. Never send job IDs, display filenames or lease tokens. Validation and approval use the existing unexpired-token checks; an expired or replaced lease cannot publish even if a delayed response is otherwise valid. A dispatch or invalid-output failure records the existing fenced processing/invalid-output failure where the lease is still current; unavailable database state grants no approval. No automatic retry loop is added; existing bounded queue attempts and operator expiry/reconciliation remain authoritative.

Only an approved opaque output ID is printed. Logs use bounded static outcome categories and never include certificate keys, tokens, input bytes, private paths or exception values. Failed jobs do not publish original or partial data.

## Evidence and delivery

Tests use owned listeners, synthetic certificates and harmless PNGs. Required evidence includes valid mutual authentication; missing, wrong-authority, expired and revoked client credentials; wrong server name/authority; unavailable authorization files; malformed/oversized/truncated/trailing frames; deadlines and admission bounds; exact Unix peer-UID denial with a healthy allowed caller; startup recovery and cancellation; and one real queue -> TLS gateway -> UID-authenticated broker -> actual Firecracker -> validation -> durable approval -> restricted reader path.

CI runs parser/property and TLS tests, real Linux broker/VM integration, existing resource/boundary/recovery suites, database permissions, browser/visual tests, migrations, restore and dependency advisories for the changed lockfile. Test probes remain separate from the decoder image. Candidate service units document identities, socket/storage permissions, TLS/authorization rotation, limits and recovery; they are development qualification examples, not deployed-production evidence.

Public HTTP attachment integration, a separate registrable media origin, dedicated-host qualification, full resource saturation, deployed routing/storage/power-loss evidence, production certificate issuance/rotation operations, and permitted visual/behavior reference collection remain prerequisites to public enablement. No compatibility parity is inferred from dispatch tests.

The user has authorized continued implementation, owned local/CI tests, focused branches, commits, pushes and draft PRs. Work uses the current checkout and verified Git identity; no merge, release or production deployment is included.

References: [Rustls client verification](https://docs.rs/rustls/latest/rustls/server/struct.WebPkiClientVerifier.html), [Tokio-Rustls](https://docs.rs/tokio-rustls/latest/tokio_rustls/), and Linux [Unix socket peer credentials](https://man7.org/linux/man-pages/man7/unix.7.html).
