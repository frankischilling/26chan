# Development dispatch service qualification

On September 9, 2026 (September 10 UTC), the actual candidate broker and gateway
units completed the owned queue-to-approval exercise on WSL Ubuntu. This extends
the [earlier direct-process evidence](verification-media-dispatch.md). Public
uploads and production execution remain disabled.

## Method and results

`scripts/test-media-dispatch.sh --systemd` runs the existing real Rust client,
gateway, broker, Firecracker decoder, SQL lease fence and restricted reader. It
creates uniquely named temporary unit files under `/run/systemd/system` from
the committed candidates. Only fixture paths and the broker dependency name are
substituted. Scripts, binaries, synthetic PKI, credentials and harmless witnesses
are in the owned fixture. No production unit is installed or enabled.

The suite passed these checks:

- The actual service PIDs run as root broker and the separate gateway account.
  Their environments retain the selected development mode and omit synthetic
  unrelated credential/canary variables. The gateway has no effective
  capabilities and has `NoNewPrivs` set.
- Actual kernel cgroup files contain the broker's 256 MiB/64-task/one-CPU limits
  and gateway's 128 MiB/32-task/one-CPU limits. The fixture supports both v1 and
  v2 layouts; the local run exercised v1 controllers.
- A harmless read succeeds under the gateway UID both outside and inside its
  mount namespace. A world-readable hidden witness returns `EACCES` inside.
  A world-writable runtime witness can be written outside before and after the
  check, but returns `EROFS` inside. The probe enters the actual service mount
  namespace and root before dropping UID/groups. This tests filesystem policy;
  it does not inherit the service's seccomp or other process restrictions.
- Production mode causes an actual new broker invocation to exit with status
  1. Both units stop, no VM or request staging remains, and restored development
  mode serves the subsequent real pipeline. Invocation/result checks reject
  unrelated start-limit or executable-launch failures as evidence.
- Real queue intake reaches approved output through mutual TLS and Firecracker.
  The restricted reader still reads the approved asset after queue deletion.
  Existing healthy credential/storage controls, revoked authorization without
  staging or approval, invalid decoded output, and expired/replaced leases all
  pass under the service profiles.
- Stopping the broker drains its dependent gateway, including cancellation
  while the probe has a live VMM. Owned processes and VM/request workspaces stop
  and disappear before fixture storage is removed.
- Unknown owned request state makes a new broker invocation fail without
  deleting that state or replacing the permanent lock inode. After the fixture
  inspects and removes only its own marker, restart completes another real
  intake, decode and approval.
- Cleanup removes recorded queue/assets, generated private files and only the
  fixture's own unit definitions. Failed or uncertain stop/VM cleanup retains
  storage and definitions for inspection.

## Failure found by the exercise

The original candidate's `ProtectSystem=strict` left `/run` writable in the
gateway mount namespace on this host. The harmless write succeeded. Captured
mount flags showed `/` read-only and `/run` read-write, with no unit drop-ins or
configured writable-path exceptions. Entering the target root as well as its
mount namespace did not change that result. The underlying platform reason was
not established; this is an observed local configuration result, not a claim
about every systemd host.

The candidate now explicitly sets `ReadOnlyPaths=/run`. The same check then
observed a read-only `/run` and rejected the write; real socket communication
and the complete pipeline still passed. The gateway only connects to the
broker's existing socket and needs no host runtime-file writes. The regression
requires `EROFS`, rather than treating any failing helper as a permission denial.

The initial entrypoint-only red run failed because its fixture module had not
yet been added. Subsequent filesystem failures above were cleaned up through
the fixture's `finally` path. No test result was accepted by weakening the unit
policy or refreshing a baseline.

## Reproduction

The local host used Linux 5.15.153.1-microsoft-standard-WSL2, systemd
255.4-1ubuntu8.17, Python 3.12.3, PostgreSQL 16.15 and pinned Firecracker/jailer
1.16.1. The native Rust build used Rust 1.94.0:

```powershell
wsl -d Ubuntu -u root -- env CARGO_HOME=/opt/26chan-rust/cargo RUSTUP_HOME=/opt/26chan-rust/rustup CARGO_TARGET_DIR=/opt/26chan-rust/target /opt/26chan-rust/cargo/bin/cargo build -p board-media-admin -p board-media-dispatch --bins --locked
wsl -d Ubuntu -u root -- env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-repro-20260909/decode.json MEDIA_VM_PROBE_CONFIG=/tmp/26chan-boundary-probe-t1wsmj04/probe.json MEDIA_DISPATCH_BIN_DIR=/opt/26chan-rust/target/debug bash scripts/test-media-dispatch.sh --systemd
```

The wrapper privately sources the existing disposable database credentials and
verifies the expected cluster. The supplied VM configurations are root-owned
mode 0600 and validated by the actual runner before intake. Accounts are
resolved and checked at runtime. A missing prerequisite fails the explicit
qualification request; it is not silently skipped.

The same command without `--systemd` also passed the complete original direct
exercise and cleanup. Shell syntax and Git whitespace checks passed. A final
inventory found no qualification units, unit files or generated `/run` fixture
directories remaining.

Fresh source review accepted the scoped service fixture, gateway restriction,
CI wiring and verification claims with no findings. It performed no additional
service or VM execution. CI runs this mode sequentially after the direct
dispatch exercise on its owned Ubuntu runner. Hosted results will be recorded
at the reviewed checkpoint.

## Remaining scope

This is disposable development service evidence with generated fixture paths
and short-lived synthetic certificates. It does not qualify a dedicated
production host, external routing, certificate operations, cold boot,
power-loss durability, full quota saturation, supervisor swap policy or public
attachment/HTTP media delivery. Reading effective kernel limits is not a
saturation test. The trusted root broker retains host authority. Forced kill
and ambiguous cleanup still require operator inspection. Production enablement
and the broader [readiness gates](readiness.md) remain unchanged.
