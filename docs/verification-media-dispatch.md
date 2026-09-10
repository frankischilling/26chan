# Media dispatch verification

The local UID-authenticated broker, bounded mutual-TLS gateway and fenced coordinator are implemented. Native qualification exercises the actual queue-to-Firecracker-to-approval path and restricted reads. Task reviews and the whole-branch review, including the scoped correction review through `f2f55fc`, are accepted with no open findings. Current-head hosted checks are a separate completion gate. These results do not qualify public uploads or production deployment.

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

## Coordinator and full native path

Implementation and native qualification are committed in `be2d689`; candidate units/configuration are in `e2566d0`, both following reviewed transport checkpoint `bb10192`.

`media-publish dispatch CLIENT_CONFIG PRIVATE_STORE` initializes the client, quarantine and output root before claiming one job. Only exact generated-ID input bytes cross the transport. Returned disk bytes are validated before the existing locked, token-and-expiry-fenced publisher runs. A 29-second outer processing timeout supplements transport deadlines without extending the 30-second lease. Processing/invalid-output failures use fenced terminal failure recording; there is no automatic retry. Publication errors retain uncertain approval state and never remove possibly approved output. Successful stdout is only an approved asset ID.

The quarantine test first failed with the missing API, then with the scaffold's `InvalidStorage` on valid input. After implementation, all nine storage tests passed. The new database/CLI integration first failed waiting for the missing dispatch command to reach an owned TLS listener. The final integration passes valid dispatch/read, invalid configuration and overlapping roots before claim, changed input length, failed transport, invalid stopped disk, and delayed valid output after expired or replaced leases. The controlled TLS server checks that its only request bytes are the exact synthetic input. One test-cleanup query initially omitted the mandatory terminal failure category; adding that fixture field fixed the constraint failure. Tests always remove only their owned IDs.

The full native harness uses actual Rust coordinator and gateway binaries, a root broker and the real reviewed Firecracker decoder. It generates private two-day PKI below an owned `/run` fixture, provisions or validates separate nologin accounts, and clears every service environment. It verifies:

- actual queue intake, authenticated dispatch, stopped-disk validation, durable approval, approved-only read, and read after queue removal;
- gateway/coordinator/VMM file denials against private keys, credentials and stores, with healthy permitted readers;
- revoked client authorization without any change to broker staging or VM workspace directories, and without approval;
- actual decoder refusal resulting in invalid output without approval;
- expiry and replacement of owned queue leases while the broker is stopped, then rejection of delayed processing after resumption;
- cancellation of the broker while the separate sleep probe has an actual live VMM, followed by stopped services and removed VM/request workspaces;
- cleanup of every owned subprocess, queue fixture and generated private file.

No fixed-byte backend stands in for the full success path. The controlled TLS database cases isolate specific coordinator failures; the separate probe supplies a live cancellation target. The restricted reader uses the coordinator filesystem UID with only `board_media_read` database credentials. Filesystem isolation from that trusted reader is not claimed. Root retains inherent host authority. The actual gateway and coordinator accounts are UID997/GID988 and UID995/GID987; VMM is UID999/GID989. Reusable harnesses resolve account names rather than relying on these local numbers.

Native full-path command, September 9, 2026:

```powershell
wsl -d Ubuntu -u root -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-repro-20260909/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-boundary-probe-t1wsmj04/probe.json MEDIA_DISPATCH_BIN_DIR=/opt/26chan-rust/target/debug bash scripts/test-media-dispatch.sh
```

The command passed all stages and its cleanup assertions. Missing native binaries, reviewed configurations, database prerequisites or idle queue state fail explicitly. The initial harness iterations exposed setup mistakes: libpq did not expand the URI in `PGDATABASE`, the private umask removed requested directory search permissions, and the publication ancestor-fsync walk needed a readable generated parent. The corrected fixture uses individually parsed PostgreSQL environment fields, explicit directory modes and private mode-0700 children. An initial decoder-error assertion expected a transport failure; the actual stopped disk correctly yields `invalid_output`, now asserted explicitly.

## Final local checks

Native Rust commands use `CARGO_HOME=/opt/26chan-rust/cargo`, `RUSTUP_HOME=/opt/26chan-rust/rustup`, `CARGO_TARGET_DIR=/opt/26chan-rust/target` and `/opt/26chan-rust/cargo/bin` first on PATH. The required database variables were sourced privately from the existing ignored files; no shared credentials were rotated.

| Command/check | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed natively |
| `cargo build --workspace --examples --bins --locked` | Passed natively and on Windows |
| `cargo test --workspace --all-features --locked` | Passed natively, including actual DB permission/concurrency/publication cases |
| `cargo test -p board-media -p board-media-admin -p board-media-dispatch --all-targets --locked` | Windows: 33 media tests, four admin tests and fourteen portable protocol/TLS tests passed; DB-only cases are covered by the native all-feature run |
| `MEDIA_DISPATCH_ROOT_TESTS=1 cargo test -p board-media-dispatch --all-targets --locked` | Five protocol and twenty TLS tests passed; all eleven native root cases executed |
| `python3 -m unittest discover -s tests/media -p test_runner.py` | Twelve passed |
| `MEDIA_DISPATCH_ROOT_TESTS=1 python3 tests/media/test_dispatch_broker.py -v` | Fourteen passed in 68.453 seconds while native compilation also ran |
| `python3 tests/media/test_vm.py -v` | Eight passed in 43.164 seconds with native `media-validate` |
| `python3 tests/media/test_boundaries.py` | Four passed, including fourteen guest denials with healthy witnesses |
| `python3 tests/media/test_recovery.py -v` | Nine passed in 31.934 seconds |
| `npm run test:behavior` and `npx playwright test --config playwright.staff.config.js` | Windows: five behavior and one staff test passed against current binaries and actual disposable DB |
| `VISUAL_FIXTURE_SERVER=1 npm run test:visual` | Three Windows snapshots passed |
| `bash scripts/test-staff-idle-migration.sh`, `test-comment-migration.sh`, `test-media-approval-migration.sh` | All three upgrade exercises passed; generated DBs removed |
| `bash scripts/restore-exercise.sh` | Sequential run passed post/asset fingerprints, fifteen counts, reader filtering and role denials; generated restore DB removed |
| `cargo audit` | No findings in 324 dependencies against 1,243 advisories |

VM commands above used the exact decoder/probe paths in the full-path command and `MEDIA_VM_VALIDATOR=/opt/26chan-rust/target/debug/media-validate`. All ran as root on the owned Linux host with a cleared environment where required. The original `scripts/verify.sh` invocation completed formatting, Clippy, build and workspace/database tests but its browser stage found Windows npm because WSL had no native Node. That child lost Linux environment variables and failed startup. Browser tests were then run in Windows with a structured private environment import and freshly built Windows binaries; no native-local browser success is claimed. One restore invocation overlapped the browser fixture writes and correctly detected a changed post fingerprint. It was rerun successfully after all browser processes stopped. Those orchestration failures are not omitted from the record.

Tool versions: Rust 1.94.0 (`4a4ef493e`, March 2, 2026), Python 3.12.3, systemd 255.4-1ubuntu8.17, Linux 5.15.153.1-microsoft-standard-WSL2, OpenSSL 3.0.13, PostgreSQL 16.15 and the pinned Firecracker/jailer 1.16.1 artifacts. One later version-only command omitted isolated rustup variables and auto-installed Rust 1.94.0 into root's default toolchain cache. It was preserved; no profiles, defaults or credentials were deliberately changed. All builds/tests reported as isolated used the explicit `/opt/26chan-rust` environment. Task3 changes only the workspace package's lockfile dependency list; it adds no registry package or registry version update.

Candidate units/configurations are committed in `e2566d0`. Native mode-0644 copies passed `systemd-analyze verify` under systemd 255, and both JSON examples parse. Actual harmless systemd probes verified the inherited-environment positive control and `env -i` preserving selected `APP_ENV=production` and the numeric UID while dropping unrelated/injected variables. Direct verification from the Windows mount initially warned about its executable/world-write modes; native copies produced no diagnostics. Candidate gateway/broker profiles themselves were not installed, enabled or run at that checkpoint. The later [actual development service exercise](verification-dispatch-services.md) records its separate results and runtime-write correction. Their paths, identities, limits, deliberate root authority and shutdown/recovery caveats are described in the [runbook](media-dispatch.md).

CI now explicitly runs the native TLS root cases, complete dispatch harness and real broker suite after existing VM qualification. The harness provisions the gateway identity before the broker suite requires it. Hosted [PR 34415517742](https://github.com/frankischilling/26chan/actions/runs/34415517742), [push 34415514993](https://github.com/frankischilling/26chan/actions/runs/34415514993) and [advisory 34415517725](https://github.com/frankischilling/26chan/actions/runs/34415517725) passed for earlier checkpoint `bb10192`; they do not establish Task3 completion.

### Review and hosted fixture corrections

The coordinator task review passed. The whole-branch review from `db60ab7` through `9d346f4` found no Critical or Important issue and one Minor evidence gap: a dispatch CLI environment-denial test could fail on its missing configuration before testing the intended guard. Commit `a9eb36d` replaces it with cases in the healthy TLS/database fixture. A successful control precedes production-mode and six unrelated-credential denials. Each denial requires a queued job with zero attempts and no lease, no accepted or pending connection, empty stdout and static stderr. Focused Windows CLI (one test), native serialized approval (one test containing the seven cases) and native CLI (two tests) passed, along with formatting.

Both Linux jobs at `9d346f4` failed. [PR run 34418362832](https://github.com/frankischilling/26chan/actions/runs/34418362832) observed a forked `timeout` child before it exec'd `sleep`; the fixture's failed assertion left its deadline monitor holding the runner lock and subsequent recovery tests failed. [Push run 34418361007](https://github.com/frankischilling/26chan/actions/runs/34418361007) could not execute the built gateway as the unprivileged test identity beneath the runner's private directory. Both Windows jobs and [advisory run 34418362819](https://github.com/frankischilling/26chan/actions/runs/34418362819) passed. Those successes do not make the failed Linux runs green.

Commit `f2f55fc` changes only the test fixtures. The TLS test copies the actual built binary through an owned private source directory into its accessible fixture, without changing runner-home or checkout permissions. The recovery test waits for actual child execution within its existing deadline and drains owned processes after a failed assertion. A controlled pre-exec observation and a forced assertion failure cover both paths. The real parent-SIGKILL check still relies on the unchanged independent external monitor.

Both reported CI failures were reproduced locally before correction: direct execution beneath the private source directory failed with permission denied, and the controlled pre-exec observation failed with `timeout != sleep`. After correction, the exact root TLS CLI test passed, the three focused recovery cases passed, and all eleven recovery tests passed in 36.982 seconds. Formatting and whitespace checks passed. No runtime, dependency or guest/service policy changed. Commands used the existing isolated Rust environment and decoder/probe paths above:

```sh
MEDIA_DISPATCH_ROOT_TESTS=1 /opt/26chan-rust/cargo/bin/cargo test -p board-media-dispatch --test tls root_gateway::production_cli_rejects_root_and_unrelated_environment_without_echoing_values --locked -- --exact
python3 tests/media/test_recovery.py RecoveryTest.test_launch_deadline_waits_for_child_exec RecoveryTest.test_launch_deadline_fixture_drains_after_failed_assertion RecoveryTest.test_launch_deadline_survives_parent_sigkill_before_any_service -v
python3 tests/media/test_recovery.py -v
```

The combined scoped rereview accepted the CLI and both CI corrections with no new or deferred findings. This is development source-review evidence, separate from independent review of deployed production boundaries. Corrected checkpoint `f2f55fc944934e8240b3061b29f06744ad8236ad` passed [PR Linux and Windows checks](https://github.com/frankischilling/26chan/actions/runs/34419926236), [push Linux and Windows checks](https://github.com/frankischilling/26chan/actions/runs/34419922950), and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/34419926142). These runs include the explicitly enabled root TLS suite, real authenticated queue-to-approval harness, broker tests, existing VM/boundary/recovery checks, browser checks, migrations and restore. This completes the implementation checkpoint; checks for later documentation-only heads are recorded in [draft PR 23](https://github.com/frankischilling/26chan/pull/23).

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

The media library passed 33 tests. Every workspace test binary reported zero failures and ignored tests. These historical native baseline checks precede the new Rust transport and do not replace the later transport/coordinator tests recorded above.

`wsl -d Ubuntu -u root -- bash scripts/restore-exercise.sh` also passed against PostgreSQL 16.15. It compared post and asset fingerprints and fifteen table counts, then checked approved-reader filtering and public/media/auth/staff grants and denials. The generated restore database was removed. The backup remains under ignored `.local/backups` per the operating procedure. This covers database recovery in the disposable cluster, not media object recovery or production backup protection.

Both hosted runs for committed broker checkpoint `1e7d5a6` passed: [push run 34411861923](https://github.com/frankischilling/26chan/actions/runs/34411861923) and [PR run 34411866431](https://github.com/frankischilling/26chan/actions/runs/34411866431). Each ran Linux application/database/browser/VM/migration/restore checks and the Windows visual job. Those recorded runs did not invoke the new broker suite. They do not cover the later TLS commits or complete authenticated dispatch; the updated CI invocation is recorded above.

## Limits

A disconnected caller may leave bounded processing underway until the runner's independent deadlines. Uncertain cleanup retains storage and exits; restart may need to wait for a surviving launch client to release its lock. The local broker has no network listener or database credentials. Its root host-launch authority remains privileged and must not be confused with the restricted guest identity.

Deployed dedicated-host routing/storage/resource controls, full saturation and power-loss tests, production certificate issuance/rotation/revocation procedures, actual candidate service deployment/shutdown behavior, public attachment and a separate registrable HTTP media origin remain unqualified. Per-attempt identities for concurrent jobs and permitted compatibility/reference evidence also remain open. The existing guest-boundary evidence is recorded separately in [media boundaries](verification-media-boundaries.md). See the [dispatch runbook](media-dispatch.md) for local use and recovery. No merge, release, production enablement or parity claim is included.
