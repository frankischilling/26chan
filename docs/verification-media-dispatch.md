# Media dispatch verification

The local UID-authenticated broker and bounded mutual-TLS gateway are implemented and reviewed. Queue-to-publication dispatch remains unfinished. These results do not qualify public uploads or production deployment.

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

## TLS gateway checkpoint

Commits `c76136f` and `2bf1791` add the Rust client and unprivileged gateway, TLS 1.3 mutual authentication, fresh bounded client authorization, fixed framing and deadlines. Windows passed five protocol and nine portable TLS/configuration tests. Native Linux passed five protocol and twenty TLS/configuration/gateway tests with `MEDIA_DISPATCH_ROOT_TESTS=1`, including all eleven explicit root integration cases. The actual CLI also ran under a nonroot identity against an owned root Unix backend.

The initial protocol, TLS and gateway scaffolds failed before implementation. Successful controls accompany authentication, revocation and identity denials. In-flight revocation tests synchronize after initial authorization and around backend processing. The TLS backend returns harmless fixed bytes; the separate broker suite exercises real VM decoding. Neither establishes the complete queue-to-publication path.

Final commands used Rust 1.94.0; native commands used the isolated environment described below:

```sh
cargo test -p board-media-dispatch --all-targets --locked
MEDIA_DISPATCH_ROOT_TESTS=1 cargo test -p board-media-dispatch --all-targets --locked
cargo clippy -p board-media-dispatch --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo audit
```

The first test command passed on Windows; the explicit root command passed on Linux. Clippy passed on both platforms. Formatting passed. Cargo-audit 0.22.2 checked 324 dependencies against 1,243 advisories with no findings. The unchanged broker suite passed all fourteen tests again in 28.997 seconds using the actual fixture paths above. An earlier invocation used nonexistent runbook example paths and failed startup; correcting the fixture paths resolved those failures without source changes.

Independent review found that the key-permission test could reject an invalid endpoint before reading the key. The fix proves construction succeeds with valid settings before changing key permissions. The same test now checks valid JSON at 65,536 bytes, then rejects one additional byte. Separate controlled mutations demonstrated that each assertion fails when its intended guard is weakened; production source was restored byte-for-byte. The focused test passed on Linux and Windows afterward, as did formatting and native Clippy. Scoped rereview accepted both corrections with no open findings. The full deadline and broker suites were not repeated for this test-only change.

Ordinary runs without explicit root enablement do not exercise the Linux gateway cases. Protected parent directories and private Windows ACLs remain operator prerequisites. Candidate deployed service behavior and production certificate lifecycle are still unqualified.

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

The media library passed 33 tests. Every workspace test binary reported zero failures and ignored tests. These native baseline checks precede the new Rust transport and do not replace its forthcoming tests.

`wsl -d Ubuntu -u root -- bash scripts/restore-exercise.sh` also passed against PostgreSQL 16.15. It compared post and asset fingerprints and fifteen table counts, then checked approved-reader filtering and public/media/auth/staff grants and denials. The generated restore database was removed. The backup remains under ignored `.local/backups` per the operating procedure. This covers database recovery in the disposable cluster, not media object recovery or production backup protection.

Both hosted runs for committed broker checkpoint `1e7d5a6` passed: [push run 34411861923](https://github.com/frankischilling/26chan/actions/runs/34411861923) and [PR run 34411866431](https://github.com/frankischilling/26chan/actions/runs/34411866431). Each ran Linux application/database/browser/VM/migration/restore checks and the Windows visual job. These workflows do not yet invoke the new broker suite; its evidence above is local. The runs do not cover the later TLS commits or complete authenticated dispatch.

## Limits

A disconnected caller may leave bounded processing underway until the runner's independent deadlines. Uncertain cleanup retains storage and exits; restart may need to wait for a surviving launch client to release its lock. The local broker has no network listener or database credentials. Its root host-launch authority remains privileged and must not be confused with the restricted guest identity.

Coordinator lease fencing through transport, full-path approval/read tests, candidate services, and deployed production qualification remain required. The existing guest-boundary evidence is recorded separately in [media boundaries](verification-media-boundaries.md). See the [dispatch runbook](media-dispatch.md) for local use and recovery.
