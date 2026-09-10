# Maintenance outcomes and alerts

The operator recorder runs a configured update command and writes its actual
outcome. A separate Rust observer reads the journal and exports failure, running
and freshness metrics. It does not execute updates. The existing advisory
workflow remains a dependency check; its success does not prove that software
was updated.

This covers configured commands only. Each command must perform the intended
update and its post-update verification before returning zero. Package
authenticity, patch selection, migration compatibility, rollback and host restart
policy remain operator responsibilities. No production updates or deployment
have been performed. [Verification](verification-maintenance-observability.md)
distinguishes local checks from required native and deployed evidence.

## Operator recorder

Install the Python 3.12+ standard-library `scripts/maintenance/run.py` and
`state.py` together under operator-owned `/opt/paperboard/maintenance`. Use
root-owned files not writable by group/other. The candidate
[service](../deploy/maintenance@.service) runs with root maintenance authority;
that authority is separate from every application and observer identity.

The positional configuration is strict JSON, at most 16 KiB:

```json
{"target":"application","state_directory":"/var/lib/paperboard/maintenance","command":["/opt/paperboard/maintenance/commands/application"],"timeout_seconds":1800}
```

Supported targets are `application`, `host`, `media_guest` and `monitoring`.
Install each actual update-and-verify command and its root-owned 0600 configuration
deliberately. No production update command is supplied by this example. Paths
must be absolute and normalized, with no symlink components. The executable is
pinned by an opened descriptor; it must be a regular executable owned by root or
the operator and not group/other writable. Config and state directory must be
owned by the effective operator, not group/other writable. Ancestors must be
root/operator owned; only root-owned sticky ancestors such as `/tmp` may be
shared-writable. Use `/var/lib` for production observer journals.

The recorder accepts 1..32 argv strings of at most 1024 bytes each and a 1..3600
second timeout. It uses no shell, runs in `/`, clears inherited environment except
fixed PATH/LANG, and discards child standard streams. Do not put credentials in
argv. Commands requiring secrets must obtain narrowly scoped operator material
through their own reviewed mechanism.

```sh
sudo /usr/bin/python3 -I /opt/paperboard/maintenance/run.py \
  /etc/paperboard/maintenance/application.json
```

An exclusive per-target lock covers running state, command execution and final
publication. Journals use bounded strict JSON, exclusive temporary creation,
file fsync, atomic rename and directory fsync. They contain only the target,
outcome, timestamps and failure flag, and are explicitly 0644 inside an
operator-owned directory. Observers must have read/traverse permission and no
write permission. Missing or malformed prior state cannot silently reset a
failed attempt to healthy.

Nonzero exit, spawn failure, timeout or termination before outcome commit records
failure. Starting a retry preserves failure until a successful completion.
Restart after an abandoned running attempt also retains a failure flag. The
recorded command leader remains unreaped while its process group is terminated,
preventing cleanup from targeting a reused leader PID. Systemd additionally
stops the whole service cgroup. This is not hostile-root containment; commands
must stay within their reviewed service and finish their own work.

Final journal publication blocks TERM/INT and checks pending termination before
committing the outcome. A termination received during a successful publication
causes a failure publication. Signals arriving after that commit do not change
the matching process exit status. A SIGKILL before final publication leaves a
running journal; timeout/freshness alerts expose it.

The candidate [timer](../deploy/maintenance@.timer) is weekly with up to 30 minutes
of randomized delay. Neither service nor timer is enabled here. Choose schedules,
successful-run age budgets, update permissions and restart windows together.
The root service has finite runtime, memory, CPU, task and stop limits; those
limits and the command's actual authority require deployed qualification.

## Read-only observer

Build `cargo build --release -p board-maintenance-monitor --locked`. The
[candidate unit](../deploy/maintenance-monitor.service) uses a separate
`board-maintenance-monitor` account, no capabilities, a read-only filesystem view
and finite resources. It needs no SQL or update credential. Configuration and
journals in production must be root-owned and not group/other writable, including
every opened directory component. All reads reject symlinks, nonregular files,
oversized input and inconsistent or future timestamps.

An observer JSON file contains one to four unique targets:

```json
{"targets":[{"target":"application","path":"/var/lib/paperboard/maintenance/application.json","max_age_seconds":691200,"run_timeout_seconds":1800}]}
```

`max_age_seconds` is 1..2592000; `run_timeout_seconds` is 1..3600. The eight-day
example allows the weekly timer's delay. An unconfigured target is outside
coverage, not implicitly healthy. Missing/denied/invalid journals remain visible
as unavailable configured targets, independently of healthy targets.

Supply root-owned `/etc/paperboard/maintenance-monitor.env` to systemd with
`APP_ENV=production`, `MAINTENANCE_CONFIG_FILE`, loopback
`METRICS_BIND_ADDR=127.0.0.1:9196` and a distinct generated 64-character lowercase
hex `METRICS_TOKEN`. The unit must be able to traverse and read its JSON source.
Startup rejects unrelated database credentials, including empty PG* variables.
Do not share its scrape token with another service.

Sampling runs every five seconds, with a two-second deadline, one blocking slot
and a thirty-second monotonic cache lifetime. A timed-out blocking operation
retains its slot. The HTTP callback reads cached memory only. Authenticated
`/readyz` is 503 if a configured target cannot be observed; a valid recorded
update failure remains readable and does not itself make the observer unready.
`/healthz` reports process liveness.

## Metrics and delivery

The `board-maintenance` job is the sixth closed job in the
[authenticated profile](authenticated-monitoring.md), with an independent token.
Each family has one closed `maintenance` label. Metrics cover sample success and
last observation, pending failure, running state/start, last successful command,
and configured freshness/runtime budgets. Missing sources expose only sample
status and the last successful observation; they do not fabricate zero failures.

Three rules have thirty-second pending periods: observer unavailable/stale,
uncleared command failure, and overdue success or running attempt. Failure and
overdue rules require a fresh valid sample and live scrape. The current
production expressions are in [alerts.yml](../deploy/monitoring/alerts.yml).
`maintenance_observer` is also available as a configured local-cgroup target in
the resource observer.

Owned Linux qualification invokes actual marker updates, failure and recovery,
uses a distinct observer identity, and checks verified-HTTPS firing/resolved
delivery. Denied and missing journals are separate cases. Production coverage
also needs actual update commands and schedules, reviewed permissions/network
policy, real operator destinations, credential rotation and independent review.

```sh
python3 -m unittest discover -s tests/maintenance -p 'test_*.py' -v
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MAINTENANCE_QUALIFICATION_PYTHON="$PWD/.local/monitor-auth-venv/bin/python" \
  bash scripts/test-maintenance-monitoring.sh \
  "$PWD/target/debug/board-maintenance-monitor" "$PWD/.local/monitoring/bin"
```

The native command requires an owned disposable Linux root session, systemd,
cgroup v2 and hash-pinned monitoring tools/venv. Repeat with
`MAINTENANCE_QUALIFICATION_INTERRUPT=1` for OS SIGTERM cleanup. It creates only an
exclusive `/var/lib/board-maintenance-monitor-<random>` fixture and exact transient
services, then verifies cleanup. Do not point qualification at a production host.
