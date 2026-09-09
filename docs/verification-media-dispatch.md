# Media dispatch verification

The local UID-authenticated broker is implemented. The Rust TLS gateway and queue-to-publication dispatch command remain unfinished. These results do not qualify public uploads or production deployment.

## Broker checkpoint

Commits `e1b3619` and `a2e821c` implement the fixed local protocol, private staging, peer-UID checks, recovery and cancellation handling. The existing runner and lifecycle modules are unchanged. Tests ran on the owned Ubuntu WSL host with Linux 5.15.153.1, the pinned Firecracker/jailer artifacts, and synthetic inputs. The dedicated nologin gateway account is UID 997/GID 988; the existing VMM is UID 999/GID 989. Tests deny a distinct UID even when it has the socket group.

The initial framing test failed because `dispatch_protocol` did not exist. A later environment regression exposed acceptance of `PGPASSFILE`; startup now uses an explicit environment allowlist. Source review found that cancellation could interrupt recording the decision to retain files after uncertain cleanup. A controlled regression failed with both signal cases before the fix and passed afterward. It exercises retention with harmless local files and a controlled executor; separate integration tests exercise actual live-VM cancellation.

Final root broker command, run September 9, 2026:

```powershell
wsl -d Ubuntu -u root -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_DISPATCH_ROOT_TESTS=1 MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-repro-20260909/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-boundary-probe-t1wsmj04/probe.json python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_dispatch_broker.py -v
```

All 14 tests passed in 30.258 seconds, with no skips. Coverage includes exact framing and deadlines, real PNG decoding, peer identity, second-broker exclusion, live SIGINT/SIGTERM cleanup, SIGKILL recovery, and retention of unknown storage. Explicit integration with missing configuration failed instead of skipping. The nonroot framing run passed six tests; root-only cases were explicitly skipped in that separate run.

Additional commands passed:

```powershell
wsl -d Ubuntu -- python3 -m unittest discover -s /mnt/c/Users/imike/4chan-rewrite/tests/media -p test_runner.py
wsl -d Ubuntu -u root -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-repro-20260909/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-boundary-probe-t1wsmj04/probe.json python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_vm.py VmTest.test_decodes_one_input_and_removes_job VmTest.test_catchable_cancellation_cleans_service_and_workspace -v
wsl -d Ubuntu -- python3 -m py_compile /mnt/c/Users/imike/4chan-rewrite/scripts/media/dispatch_protocol.py /mnt/c/Users/imike/4chan-rewrite/scripts/media/dispatch-broker.py /mnt/c/Users/imike/4chan-rewrite/tests/media/test_dispatch_broker.py
```

The first ran 12 tests, the second two; compilation completed without diagnostics. The unchanged runner did not require another broad recovery run after the broker-only fix. An independent source review identified the retention defect and accepted its scoped fix. This was an implementation review, not a deployed security audit or GitHub approval.

## Native Linux prerequisites

WSL initially had no native Rust toolchain. Rust 1.94.0, rustfmt and Clippy were installed under `/opt/26chan-rust` using the official rustup installer after checking its published SHA-256. Shell profiles and the Windows toolchain were unchanged. [Rustup documents the isolated installation settings](https://rust-lang.github.io/rustup/installation/index.html).

With `CARGO_HOME=/opt/26chan-rust/cargo`, `RUSTUP_HOME=/opt/26chan-rust/rustup` and `CARGO_TARGET_DIR=/opt/26chan-rust/target`, these commands passed:

```sh
/opt/26chan-rust/cargo/bin/cargo build -p board-media-admin --bins --locked
/opt/26chan-rust/cargo/bin/cargo test -p board-media --locked
/opt/26chan-rust/cargo/bin/cargo build --workspace --examples --bins --locked --quiet
# In the checkout, source the existing disposable test credentials privately:
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/staff.env
/opt/26chan-rust/cargo/bin/cargo test --workspace --all-features --locked --quiet
```

The media library passed 33 tests. Every workspace test binary reported zero failures and ignored tests. These native baseline checks precede the new Rust transport and do not replace its forthcoming tests. Browser comparisons, current-branch hosted checks and complete authenticated dispatch have not run for this checkpoint.

## Limits

A disconnected caller may leave bounded processing underway until the runner's independent deadlines. Uncertain cleanup retains storage and exits; restart may need to wait for a surviving launch client to release its lock. The local broker has no network listener or database credentials. Its root host-launch authority remains privileged and must not be confused with the restricted guest identity.

Mutual TLS, revocation during dispatch, coordinator lease fencing through transport, full-path approval/read tests, candidate services, and deployed production qualification remain required. The existing guest-boundary evidence is recorded separately in [media boundaries](verification-media-boundaries.md). See the [dispatch runbook](media-dispatch.md) for local use and recovery.
