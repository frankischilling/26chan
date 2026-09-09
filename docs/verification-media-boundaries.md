# Live media boundary witnesses

This checkpoint extends the existing Firecracker tests with live, synthetic destinations and private files. It advances issue #5's containment evidence without enabling uploads or changing public compatibility. It is based on media approval commit `fb05629a1d1d2ff0ec0c635bc53c88fdc297fb25` and developed on `test/media-boundary-witnesses`.

The old guest probe checked several fixed paths that might not exist on the host. The new test creates the witnesses first and proves that an allowed host context can use every one before and after the actual guest attempts access. A denied operation alone cannot qualify an unavailable witness. The guest uses the same init, UID/GID 1000, cleared environment, resource limits, Firecracker runner and stopped-output collection as the decoder; the synthetic probe remains a separate test image.

## What is exercised

| Witness | Allowed control | Actual guest operation |
| --- | --- | --- |
| Synthetic metadata service at `169.254.169.254:80` | Read the full HTTP greeting and random fixture token | TCP connection denied |
| Internal service at `192.0.2.10:8080` | Read the same bounded greeting | TCP connection denied |
| Proxy-address fixture at `192.0.2.80:3128` | Read the same bounded greeting | TCP connection denied |
| External-address fixtures at `203.0.113.10:443` and `[2001:db8::10]:443` | Read a raw TCP greeting; this is not a TLS test | IPv4 and IPv6 TCP connections denied |
| DNS at `192.0.2.53:53` and `[2001:db8::53]:53` | Send the exact `witness.invalid` query and verify its complete synthetic NXDOMAIN response | IPv4 and IPv6 UDP exchanges receive no reply; any reply would fail denial, even if malformed |
| Synthetic Unix management socket | Connect and read its fixture token | Unix-socket connection denied |
| Other-job input, private-credential and unapproved-output files | Read the token and open each existing file for writing without changing its bytes | All three reads and all three existing-file write opens denied |

This produces fourteen ordered denial results. The host checks the exact expected count, the fixed report size, every result and zero padding. Fewer or extra results, successful access, truncated/oversized output and nonzero padding fail. Previously the smoke-test parser accepted any positive count, which could hide omitted checks. Existing identity/network inspection now requires all eleven of its results as well.

All addresses belong to an anonymous network namespace created by `unshare --net`. The child verifies that its namespace differs from its actual parent's and initially contains only loopback before assigning addresses. The parent verifies its namespace and address configuration afterward. There is no veth, physical interface, default route or external service in this fixture namespace. Threads and sockets are bounded and closed, the synthetic files live in an owned temporary directory, and the normal VM helper checks that the VMM service and job workspace are gone.

These controls follow Linux's [network namespace isolation model](https://man7.org/linux/man-pages/man7/network_namespaces.7.html). The IPv4 and IPv6 fixture ranges are reserved for documentation by [RFC 5737](https://www.rfc-editor.org/rfc/rfc5737.html) and [RFC 3849](https://www.rfc-editor.org/rfc/rfc3849.html); the DNS name uses the [special-use `.invalid` domain](https://www.rfc-editor.org/rfc/rfc6761.html). No real infrastructure metadata endpoint, public DNS server or third-party Internet service is contacted.

## Commands and observed results

Local environment remains Windows Rust 1.94.0 with an owned Ubuntu WSL Linux 5.15.153.1 host, systemd 255 and cgroup v1 resource controllers. Existing Firecracker 1.16.1/jailer and guest kernel artifacts remain pinned. The new static probe was packaged into `/tmp/26chan-boundary-probe-t1wsmj04/probe.json`; its image digest is recorded in that root-owned configuration. Shared database credentials and host network configuration were unchanged.

```powershell
$env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = 'rust-lld'
cargo build -p board-media-guest --bins --example containment-probe --target x86_64-unknown-linux-musl --locked
cargo clippy -p board-media-guest --all-targets --target x86_64-unknown-linux-musl --locked -- -D warnings
cargo test -p board-media-guest --locked
cargo fmt --all -- --check
wsl -d Ubuntu -- python3 -m unittest discover -s /mnt/c/Users/imike/4chan-rewrite/tests/media -p test_runner.py
wsl -d Ubuntu -u root -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_PROBE_CONFIG=/tmp/26chan-boundary-probe-t1wsmj04/probe.json python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_boundaries.py
```

The first two report tests failed because the exact-count verifier was absent; all seven controller tests then passed. The first live-VM attempt with the prior probe returned no boundary report and failed framing validation. A deliberately malformed DNS fixture also exposed that the first helper compared a response only to what its server was configured to send; the helper now accepts a separate expected response, and the negative control fails as required. With the new guest operations, three boundary tests passed in 6.724 seconds: fourteen real denials with live controls, four rejected empty/relative-path/invalid-address/over-count requests, and rejection of malformed or stopped DNS witnesses. The static build, Linux-target Clippy, three decoder tests, formatting, Python compilation and actionlint passed.

After strengthening the child/parent namespace check and excluding advancing address-lifetime counters from the host snapshot, the same three tests passed in 6.875 seconds. The existing eight VM tests passed in 40.545 seconds with the new probe and exact-count verifier, including identity/credential checks, the live PostgreSQL control, CPU/memory/process/disk limits, cancellation and stopped-output validation. All nine recovery tests passed in 30.805 seconds. These commands used the same cleared environment and `test_vm.py -v` / `test_recovery.py -v` recipes from the [execution guide](firecracker.md), with `MEDIA_VM_PROBE_CONFIG` pointing to the new image.

For a fresh owned Linux host, build the current static binaries and use `scripts/media/provision-test.py` with a new artifact directory as described in that guide. Then run `sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_PROBE_CONFIG=/absolute/path/to/probe.json python3 tests/media/test_boundaries.py`. The script creates and verifies its own private namespace; do not invoke its internal `--isolated` entry point directly. It requires root, KVM, systemd, IPv4/IPv6 namespace support, `iproute2` and `util-linux`. Missing prerequisites fail the test rather than skipping it.

CI explicitly installs `iproute2` and `util-linux` and runs the new suite after the existing VM tests using only the test-probe configuration. Its own native Linux result must be checked at the current commit; Windows checks do not qualify this namespace behavior.

## Review follow-up: test cancellation

Source review found that the original ninety-second wrapper deadline killed only the namespace process, bypassing temporary-file cleanup and the parent's host post-check. A bounded child fixture reproduced the skipped cleanup. The wrapper and `run_probe` now supervise their own child groups, signal ordinary cancellation, allow twelve seconds for the child to unwind managed descendants (including separate-session launch monitors), and bound forced termination/reaping. The original failure still propagates. Forced termination is an error requiring inspection of retained fixture and media runner state; it is not reported as clean recovery.

The parent allocates a known private suite root and checks host namespace/address state after success, failure, timeout or handled cancellation. It removes only an empty root; uncertain contents are retained at the reported path. Allocation windows block cancellation until the child or directory is recorded. Children use [GNU `env --default-signal`](https://www.gnu.org/software/coreutils/manual/html_node/env-invocation.html) to unblock/reset INT and TERM before execution: an initial version inherited the blocked mask, and the stronger timing regression failed at 10.09 seconds. The corrected deadline and ordinary-SIGTERM cases verify cleanup markers, prompt exit and reaping of a separate-session child. A new actual-VM deadline case first observes a live VMM, then checks service termination, private-fixture removal and the host post-check after cancellation. Four boundary tests passed in 17.080 seconds; ten controller/report/cleanup/diagnostic tests passed in 2.429 seconds.

At the initial `22546cb` commit, [PR CI passed](https://github.com/frankischilling/26chan/actions/runs/34391355682), but the [push run failed](https://github.com/frankischilling/26chan/actions/runs/34391349527) in the existing repeated SIGINT/SIGTERM test because a workspace remained. Six initial local repetitions passed in 41.031 seconds. The next full VM run reproduced the failure. Safe diagnostics identified `RuntimeError at job_lifecycle.py:105` in that version: the exact process-membership check found an entry after systemd reported the service stopped. The operator diagnostics expose only the caught exception type and a trusted source filename/line, omitting exception values, input paths and locals. A fixture verifies those omissions. The test retains the bounded diagnostic on failure.

Cleanup now allows up to two seconds for that final process entry to disappear, repeating the same strict membership assertion. Only `JobProcessesRemain` is retried; unexpected errors still fail immediately. A process still present at the deadline continues to block workspace removal. The controller regression uses real short-lived and held subprocesses with a substituted membership decision to verify waiting, timeout and absence of forced deletion or termination; actual cgroup behavior remains covered by the VM suite. Its missing interface first failed, then all eleven controller/report/cleanup/diagnostic tests passed in 2.611 seconds. The complete VM suite then passed eight tests in 40.758 seconds, the boundary suite passed four in 16.890 seconds, and recovery passed nine in 30.428 seconds. This runner change addresses the observed cleanup condition separately from the test-wrapper fix. Current hosted evidence must qualify both changes before integration.

## Scope still open

These witnesses establish the listed operations against the recorded local VM profile. The external-address and proxy fixtures are synthetic TCP services in the allowed test namespace: they do not reproduce deployed routing, an Internet connection, a functioning proxy or cloud metadata semantics. The existing healthy PostgreSQL/listener test remains in the main VM suite. Production must repeat relevant denials against its actual owned application, staff, metadata, DNS, proxy and storage destinations, with healthy controls and reviewed network policy.

This does not test a guest-kernel or VMM escape, host cgroup quota saturation, power loss, hardware mitigations or a compromised trusted publisher/reader. Dedicated production host qualification, authenticated dispatch, resource saturation, deployed storage/backup/recovery, media-origin serving, attachment compatibility and independent deployed review remain required. Queue/publication fencing and operator crash recovery have separate [approval](media-approval.md) and [recovery](media-recovery.md) evidence. Public uploads remain disabled.
