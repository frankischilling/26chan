# Media job recovery

The operator runner now reconciles abandoned services and private workspaces before starting another job. `--reconcile` runs the same recovery without an input, output destination or decoder configuration. This advances M-004 under [issue #5](https://github.com/frankischilling/26chan/issues/5). It does not attach media to posts or reconcile queue leases and public storage; those remain separate requirements.

## Recovery rules

The runner, launch monitor and `systemd-run` client share the exclusive `runner.lock` descriptor. A surviving client can still submit a service request, so recovery returns failure while that descriptor remains held. The VMM does not inherit it. An external GNU `timeout` monitor bounds the launch client to 30 seconds with a two-second SIGKILL grace, independently of the Python runner. The guest service retains its separate 15-second execution deadline. Catchable cancellation kills and waits for the owned launch process group before stopping the guest service.

The short cancellation mask around process creation is inherited by the launch monitor and client. TERM can therefore require the final SIGKILL escalation. GNU timeout 9.4 unblocks its alarm signal, and the actual stalled-child test verifies that the deadline still stops both processes. See the [upstream implementation](https://github.com/coreutils/coreutils/blob/v9.4/src/timeout.c). The monitor's deadline does not prove the host service manager is healthy; unavailable service state rejects recovery.

After obtaining the lock, recovery validates the full directory and service inventory before changing either. It accepts at most 16 generated jobs in the reserved `/run/26chan-media-jobs` and `26chan-media-<32 lowercase hex digits>.service` namespace. Directories must be private, root-owned, have the existing generated name shape and contain either an approved tmpfs mount or no entries. Symlinks, ordinary files, unexpected names/permissions, nested or ambiguous mounts, nonempty unmounted directories and duplicate job identities reject recovery. The lock must be a root-owned regular file without symlinks, additional hardlinks or writable group/other permissions.

A loaded service must be transient, belong to its generated system.slice control group, and retain the expected group-kill policy, SIGKILL escalation and two-second stop timeout. Recovery stops these services, checks inactive state and zero main/control PIDs, then checks `/proc` for remaining job or dedicated-VMM processes. [Systemd's kill policy](https://github.com/systemd/systemd/blob/v255/man/systemd.kill.xml) matters: an inactive label alone cannot prove every child stopped.

Only after those checks does recovery unmount the exact validated workspace and remove its empty directory. It does not recursively delete files, detach mounts lazily, reuse a directory, open abandoned output, or approve a result. Uncertain state is retained for operator inspection. A subsequent job gets a new identity and storage.

## Operator commands

Use the host and artifact setup in [firecracker.md](firecracker.md). Run only on the owned disposable Linux processing host, with no application credentials in the environment:

```bash
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  python3 scripts/media/run-job.py --reconcile

sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-test/probe.json \
  python3 tests/media/test_recovery.py -v
```

For the current Windows/WSL checkout, the executed test command is:

```powershell
wsl -d Ubuntu -- env MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-repro-20260909/probe.json python3 /mnt/c/Users/imike/4chan-rewrite/tests/media/test_recovery.py -v
```

A busy lock is a failed recovery attempt, never permission to start another job. Allow the owned monitor/service deadlines to drain and retry. Persistent failure requires inspection of the reserved namespace and service manager. Do not remove the lock, kill unrelated processes, or force deletion over uncertain state. No database migration is needed. Deploy `job_lifecycle.py` alongside `run-job.py`; GNU coreutils `timeout` is now an explicit host dependency.

## Executed evidence

Local runs used Ubuntu WSL Linux 5.15.153.1, Python 3.12, systemd `255.4-1ubuntu8.17`, GNU coreutils 9.4 and the pinned Firecracker 1.16.1 artifacts from the previous checkpoint. All probes use synthetic input and owned services/files.

| Check | Actual outcome |
|---|---|
| Initial two SIGKILL recovery tests, before implementation | Failed: the next VM left the old workspace behind, and `--reconcile` was unavailable. Test cleanup stopped only the services/mounts created by that test |
| Independent inherited-lock probe using actual systemd-run | The client retained the lock after its parent closed the descriptor; a contender could acquire it only after client exit |
| Initial six recovery tests | Passed in 25.930 seconds |
| Added lock-symlink witness | Its first fixture setup failed when renaming between `/run` and `/tmp` filesystems. Moving the owned witness directory to `/run` fixed the fixture without relaxing the denial assertion |
| External launch-deadline test before `run_service` existed | Failed: the independent launch monitor was unavailable |
| Final recovery command above | Nine tests passed in 30.764 seconds, including the external monitor and live VMM lock-descriptor check |
| Complete `test_vm.py -v` command from the [execution record](verification-firecracker.md), after the final launch-monitor change | Eight tests passed in 40.720 seconds, including decoding, private promotion, network/resource checks, effective live cgroups, timeout and catchable cancellation |
| `wsl -d Ubuntu -- python3 -m unittest discover -s /mnt/c/Users/imike/4chan-rewrite/tests/media -p test_runner.py` | Five controller fixture tests passed |
| `python3 -m py_compile` on the runner, lifecycle helper and recovery tests | Passed |
| `actionlint .github/workflows/ci.yml .github/workflows/advisories.yml` using the local pinned executable | Passed |

The actual-VM SIGKILL test proves lock exclusion while a surviving client drains at the service deadline, automatic stale-storage recovery before the next successful VM, and absence of abandoned output collection. Another test kills the runner and its verified launch monitor/client through owned pidfds after proving the VMM is live, then verifies explicit service termination, storage cleanup and repeated recovery. A separate test kills a parent before any service exists while the same external monitor bounds a harmless stalled child with a two-second test deadline. This exercises the monitor, not an injected hang in the actual systemd request path.

Other checks retain outside witness files and uncertain workspace/service state, refuse recovery during an active runner, reject a lock symlink, remove valid prelaunch/stopped workspaces, and preserve nested mounts and unexpected service policies. These are real filesystem/process/service checks on the recorded host. They are not guest escape tests or a production security audit.

Read-only source review approved the initial reconciliation and the independent launch monitor follow-up, with no Important or Critical findings. The reviewer did not run the tests. CI includes both the existing VM suite and these recovery checks and must establish its own hosted result.

Remaining work includes authenticated queue dispatch, publication fencing and database/filesystem crash reconciliation, deployed restart/power-loss exercises, maintained production host qualification, the wider network/storage/resource evidence, complete media compatibility and independent deployed review. Public and production media enablement remain rejected.
