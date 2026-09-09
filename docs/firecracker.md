# Local isolated media execution

This profile runs one job in Firecracker 1.16.1 with its matching jailer. A small Rust init program starts the PNG decoder as guest UID/GID 1000, without capabilities, environment variables or inherited application descriptors. The guest has one read-only input disk and one fixed writable output disk. It has no network interface, vsock, Firecracker API socket or host directory share.

The Python runner is an operator deployment utility for an owned disposable Linux host. Neither web application calls it. It has no database connection, authenticated service endpoint or production queue integration. Public media enablement remains rejected. The [execution verification](verification-firecracker.md) and [job recovery record](media-recovery.md) distinguish executed tests from remaining qualification work.

## Build and run

Use the disposable database setup in the [README](../README.md) first. The VM tests require its real PostgreSQL listener on port 55432 as a healthy positive control. The host needs Linux x86_64, working KVM, systemd, GNU coreutils `timeout`, effective memory/CPU/pids cgroup controllers, Python 3.12, and root permission to create private tmpfs mounts and services. The provisioner creates a dedicated `board-media-vmm` system account with no login shell and refuses to reuse its artifact directory.

In Linux:

```bash
cargo build -p board-media-admin --bin media-validate --locked
rustup target add x86_64-unknown-linux-musl --toolchain 1.94.0
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
cargo build -p board-media-guest --bins --example containment-probe \
  --target x86_64-unknown-linux-musl --locked
sudo python3 scripts/media/provision-test.py /tmp/26chan-media-test \
  "$PWD/target/x86_64-unknown-linux-musl/debug"
python3 -m unittest discover -s tests/media -p test_runner.py
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-test/decode.json \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-test/probe.json \
  MEDIA_VM_VALIDATOR="$PWD/target/debug/media-validate" \
  python3 tests/media/test_vm.py
```

On this Windows checkout, build with PowerShell:

```powershell
cargo build -p board-media-admin --bin media-validate --locked
rustup target add x86_64-unknown-linux-musl --toolchain 1.94.0
$env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = 'rust-lld'
cargo build -p board-media-guest --bins --example containment-probe --target x86_64-unknown-linux-musl --locked
Remove-Item Env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER
wsl -d Ubuntu -- python3 /mnt/c/Users/imike/4chan-rewrite/scripts/media/provision-test.py /tmp/26chan-media-test /mnt/c/Users/imike/4chan-rewrite/target/x86_64-unknown-linux-musl/debug
wsl -d Ubuntu -- env MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-test/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-test/probe.json MEDIA_VM_VALIDATOR=/mnt/c/Users/imike/4chan-rewrite/target/debug/media-validate.exe MEDIA_VM_TEST_TEMP=/mnt/c/Users/imike/4chan-rewrite/.local python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_vm.py
```

The optional `MEDIA_VM_TEST_TEMP` puts private test results on the Windows drive so the Windows validator can perform atomic file operations there. WSL UNC paths did not work for that validator in this environment. Native Linux uses its ordinary temporary directory. Adjust checkout paths when using another location. Keep application credentials out of the runner environment.

For a single operator job, create a private directory for the stopped disk and a separate sibling directory for approved results. Run the job with the decode configuration, then run `media-validate OUTPUT_DISK PRIVATE_RESULT_DIRECTORY`. The runner's zero exit status establishes service termination only. The Rust validator must accept the entire disk before any result is approved. Its generated PNG filename and receipt come from the host. Invalid output creates no approved directory; originals are never copied to approved storage. No public download route exists.

## Boundaries and limits

| Boundary | Implemented local policy |
|---|---|
| Operator utility | Root only, production mode rejected, credential-bearing environment names rejected; subprocess environment is explicitly cleared |
| Host VMM | Dedicated `board-media-vmm` identity, matching jailer, new PID and mount namespaces, private network namespace, no API socket, one serialized job |
| Job filesystem | Fresh generated directory below root-only `/run/26chan-media-jobs`; 96 MiB tmpfs includes its artifacts and disks; no guest filesystem is mounted by the host |
| Input | Big-endian u64 byte count, 1 through 8 MiB, exact bytes and sector padding; guest disk read-only |
| Output | Exactly 4,194,816 bytes: existing IBRGBA01 header/pixels and all-zero padding; maximum 1,024 by 1,024 pixels |
| Guest resources | One vCPU, 128 MiB physical memory; worker 96 MiB address space, five CPU seconds, 16 tasks per UID, 32 file descriptors, no core dumps |
| Host resources | 256 MiB memory budget, 100% CPU quota, 32 tasks, 64 descriptors, bounded file size; effective hierarchy controls are checked before guest execution |
| Time | 15-second service deadline and two-second stop grace; separate 30-second launch-client monitor with two-second kill grace; cancellation stops the launch group and service before removing storage |
| Output collection | Host captures the output inode before VM startup, reads after termination, checks actual size and bounds its copy; Rust rechecks dimensions, every padding byte and EOF |
| Private promotion | Existing bounded PNG encoder and atomic no-clobber storage; no worker commands, paths, filenames, archives or success flags are accepted |

The service-side `verify-cgroups.py` helper checks the generated service's actual controller membership and effective memory, CPU and task limits before executing jailer. On cgroup v1 it also sets and verifies a 256 MiB combined resident-plus-swap cap and swappiness zero. That is not an absolute prohibition on swapping under global host reclaim. Cgroup v2 requires a separate zero-swap limit. Missing or ineffective controls reject execution. Production must review host swap/data-remanence policy along with current kernel and hardware mitigations. Do not infer those properties from an accepted systemd setting.

The local decoder accepts still PNGs up to 1,024 pixels per dimension, including RGB, RGBA and grayscale conversions. Other formats, larger source images, animation, thumbnail policy and original downloads remain compatibility work. These local limits are not advertised as the finished public attachment contract.

## Failure and maintenance

Ordinary failure, timeout and supported cancellation stop the service before unmounting and removing its job directory. No job directory is reused. After SIGKILL, the next runner or the operator's `--reconcile` command validates and reconciles abandoned services and mounts under the same exclusive lock. Surviving launch clients retain exclusion and have an independent external deadline. Unknown storage/service state blocks new work. See [recovery commands and evidence](media-recovery.md); deployed restart and power-loss exercises remain required.

[firecracker-artifacts.json](firecracker-artifacts.json) pins download URLs and hashes. The provisioner verifies them and records hashes of the built initramfs and extracted runtime binaries in its private configuration. The CI kernel comes from upstream demonstration artifacts; it is not an approved production image. For updates, review the upstream release/kernel guidance, replace the pins deliberately, rebuild both guest binaries, and repeat the VM and host validator tests. Keep test probes out of deployed decoder images.

Remaining requirements include a maintained dedicated processing tier; a separately authenticated bounded queue-dispatch interface; per-attempt identity allocation for concurrent jobs; deployed restart qualification; database/filesystem publication fencing; healthy positive controls for metadata, DNS, other jobs and unauthorized storage; full resource saturation qualification; media-origin serving; complete format compatibility; and independent security review. Local results do not satisfy those unfinished requirements.
