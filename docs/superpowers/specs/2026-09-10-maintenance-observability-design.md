# Maintenance outcome observation

Prompt section 11 requires alerts for update failures. The scheduled advisory
workflow checks known dependency advisories; it does not establish that updates
ran or succeeded. Add an operator-run command recorder and a separate Rust
observer, with real command and authenticated alert qualification. Production
update commands, schedules and deployed privileges still require qualification.

## Authority and approach

The operator recorder runs one fixed argv from an operator-owned configuration.
It records the actual command result in a bounded atomic journal. The command
must perform its update and post-update checks before returning success. The
recorder does not select updates, trust advisory CI as update evidence, install
packages automatically, expose an HTTP execution endpoint, or accept commands
from the application. Its command authority is maintenance authority and may be
broad for host updates; it is never granted to the observer or web services.

The observer reads only configured journal files and serves cached fixed metrics
through the existing private bearer endpoint. It has no database client, update
command, service-control credential or outbound HTTP client. A separate process
keeps maintenance source failure from invalidating resource observations.

Alternatives considered: public workflow polling only observes CI checks and
adds outbound API identity/freshness complexity; a textfile-only producer without
real command execution would not establish update outcomes. The journal approach
provides an actual producer and keeps observation read-only.

Closed target order is `application`, `host`, `media_guest`, `monitoring`.
Only configured targets appear; absence from configuration is explicitly outside
coverage. No paths, argv, exit output, user identifiers or secrets become labels.

## Journal and producer

Python 3.12+ standard-library operator tooling follows the existing repository
operations tools. `scripts/maintenance/run.py CONFIG` supports Linux only and
uses this strict JSON configuration, at most 16 KiB:

```json
{"target":"application","state_directory":"/var/lib/paperboard/maintenance","command":["/opt/paperboard/maintenance/application"],"timeout_seconds":1800}
```

Reject unknown/duplicate fields, noncanonical or relative paths, symlink path
components, nonregular configuration, configuration/directory not owned by the
effective operator UID or writable by group/other, invalid target, and invalid
argv. Require 1..32 argv strings, each nonempty, at most 1024 bytes, without NUL;
the executable must be an absolute canonical regular file. Timeout is 1..3600
seconds. The caller supplies no inherited environment to the child except fixed
PATH and LANG. Run with cwd `/`, no shell, null standard streams and a new session.
Do not print configuration, command, paths or child output on errors.

Hold an exclusive nonblocking flock on the fixed `<target>.lock`, validated as
an operator-owned regular file, through command execution and final publication.
Open source and output paths without following symlinks, including components;
hold the state directory descriptor for relative file operations. Publish
`<target>.json` by exclusive temporary creation, file fsync, atomic rename and
directory fsync. Files are 0644: the journal contains only the following fields,
and its directory is operator-owned and not writable by observers.

```json
{"schema":1,"target":"application","started_ms":1789056000000,"finished_ms":1789056001000,"outcome":"success","last_success_ms":1789056001000,"failure_pending":false}
```

Every field is required; `finished_ms` and `last_success_ms` may be null.
Timestamps are positive integer UTC milliseconds below 2^53. Outcomes are
`running`, `success`, `failure`. Running has null finished time; completed has
finished >= started. Success requires last_success == finished and no pending
failure. Failure requires pending failure and preserves prior last success.
Running preserves prior last success and pending failure. Prior success must
not be later than a new attempt start. Clock rollback rejects a new attempt or
completion without inventing timestamps. Invalid existing state is rejected,
not reset to healthy. An abandoned prior running attempt latches failure when a
new lock holder starts; only a completed successful command clears it.

Publish running before spawning. Spawn failure, nonzero exit, timeout or SIGTERM/
SIGINT records failure. A killed recorder leaves running state, which becomes
overdue. Use monotonic command deadlines; keep the child leader unreaped with
waitid(WNOWAIT) until its owned process group has been stopped, then reap it, so
cleanup cannot target a reused leader PID. A candidate systemd unit additionally
uses KillMode=control-group and finite stop/runtime/resource limits. Trusted
maintenance commands must remain in the service and complete their own checks;
this recorder is not containment for hostile root maintenance code.

## Observer and metrics contract

`apps/maintenance-monitor` is safe Rust, Linux-only, reusing locked dependencies.
Required APP_ENV is development or production; require MAINTENANCE_CONFIG_FILE
and the existing METRICS_BIND_ADDR/METRICS_TOKEN. Reject all unrelated known
database environment variables and PG* variables even when empty/non-Unicode.
Configuration <=16 KiB has one to four unique targets:

```json
{"targets":[{"target":"application","path":"/var/lib/paperboard/maintenance/application.json","max_age_seconds":604800,"run_timeout_seconds":1800}]}
```

Require strict fields, canonical absolute paths, max_age_seconds 1..2592000 and
run_timeout_seconds 1..3600. Production configuration and journal files must be
root-owned and not group/other writable. Open every component without symlinks;
read only regular nonblocking files <=4096 bytes. Missing, denied, malformed,
wrong-target, inconsistent or future-dated journals are unavailable, never zero
failure. Valid files are sampled independently: one missing target does not hide
the others. Production directory ownership and write authority are deployment
prerequisites, with actual distinct-UID denials in the owned CI fixture.

Sample every 5 seconds, blocking admission one, deadline 2 seconds, monotonic
cache staleness 30 seconds, bounded runtime shutdown. Hold the blocking permit
inside the operation after timeout; reject late ready results by elapsed time.
Metrics callbacks read memory only. Health is process liveness; authenticated
readiness is 503 if any configured target cannot be observed. A valid recorded
maintenance failure is observable and does not make the observer unready.

In `board-observe`, add `MAINTENANCE_TARGETS: [&str;4]` in the order above;
`MaintenanceSample { targets: [Option<MaintenanceTargetSample>;4] }` and
`MaintenanceTargetSample` with fields `available: bool`,
`last_sample_timestamp_seconds: u64`, `running: bool`, `failure_pending: bool`,
`started_ms: u64`, `last_success_ms: u64` (0 means no success),
`max_age_seconds: u64`, `run_timeout_seconds: u64`.
Expose `Metrics::register_maintenance(Fn() -> MaintenanceSample + Send + Sync + 'static)`.

All families have exactly one `maintenance` label with a closed value:

- board_maintenance_sample_success
- board_maintenance_sample_last_success_timestamp_seconds
- board_maintenance_run_in_progress
- board_maintenance_failure_pending
- board_maintenance_run_started_timestamp_seconds
- board_maintenance_last_success_timestamp_seconds
- board_maintenance_max_age_seconds
- board_maintenance_run_timeout_seconds

Only the first two are emitted for an unavailable configured target. Millisecond
values render as fractional seconds. Unconfigured targets emit nothing.

## Alerts and profile

Add private scrape job `board-maintenance` on candidate port 9196 with independent
token; extend generated profile closed jobs. Add `maintenance` to Alertmanager
grouping and `maintenance_observer` as the eleventh closed resource service
target. Existing resources and metrics retain their meaning.

Three rules, all with 30-second pending periods:

- BoardMaintenanceObserverUnavailable: scrape down or configured sample missing,
  unsuccessful or older than 30 seconds.
- BoardMaintenanceFailed: healthy fresh sample with failure_pending == 1.
- BoardMaintenanceOverdue: healthy fresh sample with no previous success, last
  success older than max_age, or a running attempt older than run_timeout.

Gate outcome alerts on successful fresh sampling and live scrape. Test healthy,
failure/retry persistence/recovery, each overdue cause, stale/unavailable,
per-target independence and equality boundaries. Do not report update availability
or patch completeness: the recorded result covers the configured command only.

## Qualification and delivery

Portable Rust/Python parser and state tests cover invalid input and transitions.
Linux CI runs actual producer success, nonzero exit, timeout, signal interruption,
exclusive lock and restart after killed runner with owned benign children. A real
fixture update replaces a version marker inside its owned directory and verifies
it, then fails on unavailable input and recovers after restoration. Tests invoke
the producer; they do not write invented success/failure journals as outcome proof.

A separate observer identity reads journals but cannot modify journal/config,
read an operator payload or execute the update payload. The generated authenticated
Prometheus/Alertmanager profile observes real failure, overdue and missing-source
firing/resolved pairs. Require current generation and exact labels/fingerprint/
startsAt correlation and native healthy boundaries. Test normal and OS SIGTERM
cleanup of exact owned services, children, listeners and temporary credentials.
Time constants may accelerate in the owned qualification, with production values
and conditional logic preserved. No production updates or deployment occur.

## References and limits

[Python subprocess](https://docs.python.org/3.12/library/subprocess.html) describes
explicit argv/environment/session handling; [os](https://docs.python.org/3.12/library/os.html)
documents descriptor-relative I/O and waitid/WNOWAIT. Pin actual runtime/tool
versions in verification. The wrapper records command outcomes, not package
authenticity, rollback safety, host patch completeness or deployed boundaries.
Those remain operator checks and launch prerequisites.
