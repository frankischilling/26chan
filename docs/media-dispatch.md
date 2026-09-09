# Authenticated media dispatch

The local development broker accepts one bounded media input from one configured Linux UID, runs the existing isolated decoder, and returns its stopped output disk. The caller must still validate that disk before publication. This checkpoint supplies the local broker; remote mutual TLS, queue dispatch and deployed qualification are separate work.

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

This is development evidence. It does not qualify remote TLS, queue lease/publication fencing, public uploads, a dedicated processing host, deployed service operation, power-loss recovery or a separate media origin. The broader remaining requirements are recorded in the [dispatch design](superpowers/specs/2026-09-09-media-dispatch-design.md).
