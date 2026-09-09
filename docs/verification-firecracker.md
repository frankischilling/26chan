# Firecracker verification

This checkpoint advances media requirements M-003/M-004 under [issue #5](https://github.com/frankischilling/26chan/issues/5). It executes a Rust PNG decoder in a real disposable Firecracker guest and validates the stopped raw output with the existing bounded Rust parser and PNG promoter. Approval in these tests writes only private fixture storage. Public attachments, authenticated queue dispatch and production publication are unfinished.

## Environment and artifacts

Local runs used Windows Rust 1.94.0, Ubuntu WSL Linux 5.15.153.1, systemd 255, Python 3.12, working KVM API 12 and hybrid cgroups with v1 memory/CPU/pids controllers. A benign `KVM_CREATE_VM` call succeeded. The WSL host has enabled swap and is not a qualified production processing host.

Firecracker and jailer 1.16.1 came from the official release. The archive matched its published SHA-256. Guest Linux 6.1.186 and its configuration came from the official getting-started guide's CI artifact bucket. [The manifest](firecracker-artifacts.json) records source URLs, hashes and provenance; the CI kernel is demonstration material. The provisioner packages only the built static Rust init and either the decoder or a separate synthetic test probe. Each private configuration records the built artifact hashes.

The actual VMM uses the dedicated local `board-media-vmm` account (UID 999/GID 989 on this host); the decoder runs as guest UID/GID 1000. No application credential is passed to either. The operator utility holds root host authority and is not callable by public/staff routes. The normal public dependency graph does not include the guest crate.

## Commands and outcomes

| Command/check | Actual outcome |
|---|---|
| `cargo test -p board-media --test block --locked` before Task 1 | Expected missing-interface compile failure; later nine protocol tests passed |
| `cargo test -p board-media-guest` against the initial decoder stub | Two expected decoder-unavailable failures, one rejection test passed |
| `cargo test -p board-media-guest --locked` | Three decoder tests passed after implementation |
| `cargo build -p board-media-guest --target x86_64-unknown-linux-musl --locked` without an explicit Windows cross-linker | Failed: `cc` unavailable; setting `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` fixed the build |
| `cargo build -p board-media-guest --bins --example containment-probe --target x86_64-unknown-linux-musl --locked` with that linker | Passed; static Linux init, decoder and probe built |
| `cargo test -p board-media-admin --test validate --locked` | Initial compile error used `ObjectId::new` instead of `generate`; after correction the validator/promotion test passed |
| `cargo test -p board-media-guest -p board-media --locked` | 31 tests passed |
| `cargo clippy -p board-media-guest --all-targets --target x86_64-unknown-linux-musl --locked -- -D warnings` | Passed, including Linux-only guest initialization |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo build --workspace --examples --bins --locked` | Passed |
| `cargo test --workspace --all-features --locked` | 114 tests passed, zero failed or ignored, using actual disposable public/media/auth/staff PostgreSQL credentials |
| `python -m py_compile scripts/media/run-job.py scripts/media/build-initramfs.py scripts/media/provision-test.py tests/media/test_vm.py` | Passed |
| `wsl -d Ubuntu -- python3 scripts/media/provision-test.py ...` using the full checkout paths in the runbook | Fresh collection at `/tmp/26chan-media-repro-20260909` succeeded; downloads verified and per-image configurations written |
| Initial actual VM test against the unavailable runner | Failed as expected: no isolated runner existed |
| First real guest boot | Guest decoded and halted, but generic ACPI poweroff did not exit the VMM; external service deadline killed it. Guest reset with `reboot=k` fixed normal termination |
| Actual worker access test | Found writable initramfs root. Explicit root mode 0755 fixed the same denial assertion |
| Initial complete VM suite described below | Six tests passed in 30.916 seconds. Review then found the host cgroup-v1 swap-control and catchable-cancellation gaps described below |
| New cancellation/live-limit regressions before the fix | Two tests reported three failures: both termination cases retained a workspace, and actual v1 combined memory/swap remained effectively unlimited |
| Final complete VM command below with `-v` | Eight tests passed in 40.324 seconds after the runner fixes |
| Final focused cancellation/live-limit command below | Two tests passed in 9.640 seconds after the final test cleanup adjustment |
| Initial hosted CI at `e80bf6a`: [push](https://github.com/frankischilling/26chan/actions/runs/34376082969), [PR](https://github.com/frankischilling/26chan/actions/runs/34376106465) | Both runs passed application/database/browser checks, the historical staff migration and seven VM tests, but the cancellation test errored before its first VM started: it enumerated a job root that the first runner had not created yet. Later comment-migration/restore steps were skipped by that failed step |
| Fresh-host cancellation reproduction | Removing only the verified idle test lock and empty root reproduced both `FileNotFoundError` cases locally. The readiness poll now treats an absent root as not ready within its existing eight-second deadline; early runner exit and cleanup assertions still fail normally |
| Complete VM command with `-v` after that polling fix, starting with no job root | Eight tests passed in 40.578 seconds. A scoped source review found no weakened assertions; the hosted rerun must establish its own result |
| `wsl -d Ubuntu -- python3 -m unittest discover -s /mnt/c/Users/imike/4chan-rewrite/tests/media -p test_runner.py` | Five effective-control fixture tests passed; actual v1 verification is additionally covered by the live VMM test |
| Windows validator via WSL UNC paths | Failed; the same actual pipeline using an ignored C-drive fixture directory passed. The test runbook sets `MEDIA_VM_TEST_TEMP` explicitly |
| `cargo audit` | Passed: 312 locked dependencies checked against 1,243 known advisories |
| `npm.cmd audit --audit-level=moderate` | Passed: zero reported vulnerabilities |
| `.\.local\tools\actionlint.exe .github/workflows/ci.yml .github/workflows/advisories.yml` | Passed, including the new owned-runner media job step |
| `wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/check-launch-readiness.sh` | Expected exit 1: full compatibility, production media/staff boundaries and deployed operations remain incomplete |

The initial complete VM command was:

```powershell
wsl -d Ubuntu -- env MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-repro-20260909/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-repro-20260909/probe.json MEDIA_VM_VALIDATOR=/mnt/c/Users/imike/4chan-rewrite/target/debug/media-validate.exe MEDIA_VM_TEST_TEMP=/mnt/c/Users/imike/4chan-rewrite/.local python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_vm.py
```

The final complete run used that command with `-v`. The final focused command was:

```powershell
wsl -d Ubuntu -- env MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-repro-20260909/probe.json python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_vm.py VmTest.test_catchable_cancellation_cleans_service_and_workspace VmTest.test_live_vmm_has_effective_host_resource_limits -v
```

## What the local probes establish

The normal guest decoded a synthetic one-pixel PNG to exact RGBA output. After guest termination, the actual Rust validator accepted that disk and produced one private PNG. Changing a padding byte caused rejection before creating another approval directory. Library tests separately cover maximum dimensions, oversized/truncated/trailing data and bounded consumption.

The probe ran under the same guest initialization and unprivileged identity as the decoder. It observed an empty environment, zero effective capabilities, no-new-privileges, null standard descriptors and only a loopback network interface. It could not open the listed host-management/other-device paths, write its read-only input, or create a file in the root filesystem. TCP connections to the owned test listener and PostgreSQL listener failed. An allowed host context read the TCP service's greeting and confirmed PostgreSQL readiness before and after the guest test. The greeting service is raw TCP, not HTTP.

Guest tests exercised address-space allocation denial, process-count denial after successful child creation, refusal to write past the fixed output device, and CPU termination before the probe's independent deadline. The service's external wall deadline stopped a sleeping guest; tests checked that its generated service and job directory were gone. These guest limit checks are not measurements of host cgroup memory/CPU saturation.

The live VMM test checked its actual memory/CPU/pids cgroup membership after jailer execution. Resident memory and combined resident-plus-swap limits were both 268,435,456 bytes, memory hierarchy accounting was enabled, swappiness was zero, CPU quota equaled its positive period, and pids.max was 32. The service-side helper verifies those controls before guest execution and rejects absent or ineffective controls. The v2 helper path has filesystem-fixture coverage locally; an actual v2-host VM result must come from its own run. Global host swap was unchanged.

Actual SIGTERM and repeated SIGINT tests now unwind through full service cleanup and permit a subsequent successful job. After testing, there was no generated service or VMM process and the job directory contained only its runner lock. This does not cover SIGKILL or power loss.

## Review and remaining evidence

The protocol task's separate review approved its spec compliance and quality. A broader source review found two issues in the initial runner: systemd `MemorySwapMax=0` did not apply to the WSL v1 memory controller, and ordinary SIGTERM bypassed Python cleanup. Commit `748b89a` adds effective cgroup verification before guest execution and protected cancellation cleanup with the regression results above. It retains the VMM in the generated systemd service subtree. The initial six passing tests alone did not prove either property.

A scoped source re-review marked both findings addressed and found no new Important issues in the fix. These are implementation reviews, not a production security audit or a fabricated GitHub approval.

The existing registry dependency versions did not change. The public/staff handlers, database schema/grants and screenshot baselines did not change. Local browser suites and historical migration/restore exercises were not repeated for this slice; CI retains those existing checks. No production deployment, release or merge was performed.

Unrun or incomplete requirements include a maintained dedicated production host; hardware/kernel mitigation review; complete metadata/DNS/Internet/other-job/unauthorized-storage controls with healthy allowed-context witnesses; external quota-saturation qualification; SIGKILL/host-crash orphan reconciliation; authenticated coordinator dispatch; queue lease and filesystem publication fencing; complete media formats/thumbnail/original-download compatibility; public media-origin serving; monitoring/alerts; protected production backups/restore; and independent deployed security review. These remain required work on the full rewrite.
