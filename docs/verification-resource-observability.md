# Resource observability verification

Work for [issue #40](https://github.com/frankischilling/26chan/issues/40) starts
from main `3649d15d997d2cb9e9184c444a44d22b0346e9c6`; design/plan commit `7bb3445`.
Initial implementation `6b6a819c7ef72dab41a49fa540ce0f78de40f95a` passed all seven
hosted checks. A later check exposed a qualification defect in alert-cycle
association, described below; the initial results do not replace the strengthened
notification checks on the final revision.
[PR #41](https://github.com/frankischilling/26chan/pull/41) tracks the final
revision, its checks and the authorized merge. Production
deployment and qualification remain separate requirements.

Portable checks on Windows, September 10, 2026:

- The strict configuration and collector parsers first failed against stubs.
  The completed crate passed 16 tests, including arbitrary input parsing,
  environment rejection, path validation, cache age/failure/recovery and bounded
  blocking admission. All-target Clippy passed with warnings denied.
- Independent runtime review found a late ready result could be accepted after
  its deadline. The regression failed against that implementation, then passed
  with an elapsed monotonic deadline check. A separate review finding led to
  channel-gated recovery testing so a busy runner cannot miss the failure state.
- Linux-musl library typechecking passed without linking or WSL execution.
  Subsequent hosted Linux execution passed all 19 resource crate tests, including
  actual directory statistics and rejection of symlinks, FIFOs, disappeared
  sources and fabricated cgroup files on an ordinary filesystem.

Independent parent checks passed all 28 board-observe tests, all 20 resource-rule
cases and both existing rule suites, native validation of all 18 rules, ten
monitoring helper tests, and ten profile tests (one POSIX-only skip on Windows).
Workspace formatting and diff whitespace checks passed. The lockfile changes
only the new local package. Separate read-only review found no further blocker
in configuration/collection after verifying the sampler corrections. Independent
integration review found no additional blocker in the helper, service unit or CI;
two stale operations descriptions were corrected. The implementation reviews and
their two corrected runtime/test findings are separate from production security
review, which remains required.

Initial implementation `161945f` passed hosted Windows visuals and the advisory
scan. Linux CI stopped at `clippy::non_octal_unix_permissions` in a Linux-only
test. The permission value was changed from `0` to `0o0`; its value and assertions
are unchanged. Native pressure qualification had not run at that point.

The documentation-only revision `fd6ec72` passed its
[push build](https://github.com/frankischilling/26chan/actions/runs/34502392944),
but its [PR build](https://github.com/frankischilling/26chan/actions/runs/34502397131)
failed during CPU-throttling qualification after byte, inode, memory and task
cases passed. The buffered log lacked sub-phase detail, so the exact failing
assertion could not be established from that run.

Independent behavioral reproduction identified a concrete matcher defect:
delayed messages from an earlier same-label CPU activation could select an old
resolution before a valid current one, or let a complete old pair satisfy the
later stage. A healthy Prometheus rule observation and a one-time receiver-queue
drain do not constitute a notification-delivery barrier. Earlier memory/task
work can itself cause CPU throttling in the same finite cgroup.

The strengthened healthy control requires both Prometheus rule absence and
authenticated Alertmanager absence for the owned alert. This prevents a new
activation from overlapping a still-current prior alert: pinned Alertmanager
0.34.0 retains the earliest start time when merging overlapping alerts. Its
native alerts API excludes expired alerts and includes suppressed alerts when
the corresponding filters are enabled. See the pinned
[merge implementation](https://github.com/prometheus/alertmanager/blob/v0.34.0/alert/alert.go)
and [API filter](https://github.com/prometheus/alertmanager/blob/v0.34.0/api/v2/api.go).

The checks require a firing activation at or after the current
stage's healthy boundary, then select its resolution by exact nonempty start
time, fingerprint and labels. Missing or invalid start times cannot qualify.
Delayed old messages are skipped while waiting for the current generation.
Closed sub-phase diagnostics and failure-only allowlisted CPU scalars identify
future failures without dumping native logs, private configuration or credentials.
Thresholds, native pressure checks and pressure/delivery deadlines remain
unchanged. The combined healthy control uses the existing 55-second limit;
the source-unavailable case now shares that bounded healthy wait.
The PR records the corrected revision and its required native reruns; the defect
is a plausible explanation of the historical failure, not a proven attribution.

The correction passed 13 resource qualification helper tests, including the
failure-first generation regressions and a real local TLS 1.3 server check for
API credentials, wrong-CA rejection, exact filters, all current alert states,
malformed responses and healthy recovery. The full monitoring helper suite
passed 18 tests. Python compilation, shell syntax and diff whitespace checks
also passed. These local checks do not replace the corrected native Linux runs.

## Initial hosted implementation evidence

All these runs checked `6b6a819` and completed successfully:

| Check | Run |
|---|---|
| PR Linux application/native checks and Windows visuals | [34500076206](https://github.com/frankischilling/26chan/actions/runs/34500076206) |
| Push Linux application/native checks and Windows visuals | [34500071868](https://github.com/frankischilling/26chan/actions/runs/34500071868) |
| PR HTTP, queue/resource rules and authenticated monitoring | [34500076165](https://github.com/frankischilling/26chan/actions/runs/34500076165) |
| Push HTTP, queue/resource rules and authenticated monitoring | [34500071839](https://github.com/frankischilling/26chan/actions/runs/34500071839) |
| Dependency advisories | [34500076184](https://github.com/frankischilling/26chan/actions/runs/34500076184) |

The PR runner reported Ubuntu 24.04, kernel `6.17.0-1022-azure`, systemd 255
(`255.4-1ubuntu8.17`) and cgroup2fs. Its 19 resource tests passed with no skips.
The full workflow also passed existing application/database/concurrency/browser
checks, media guest/dispatch/reader boundaries, migration upgrades, restore and
queue alert qualification. The advisory check scanned 328 locked dependencies
against 1,243 fetched advisories; Cargo and npm checks reported no findings.
That is a known-advisory result, not a complete transitive security audit.

Both Linux resource runs exercised an exclusively owned 16 MiB/256-inode tmpfs,
a 64 MiB/no-swap/16-task/20%-CPU fixture cgroup, and a distinct DynamicUser
observer. Before exec and after a healthy scrape, actual operations established
statistics access and denied protected-payload reads, creation/modification and
cgroup-limit writes. Root-owned payload and control operations supplied healthy
positive controls. No host root filesystem or global memory pressure was used.

The real Rust measurements matched the native finite limits. Missing/wrong scrape
and native API credentials were denied. Native Prometheus and Alertmanager
validated and used the generated verified-HTTPS profile. All eight cases passed:

| Source condition | Actual observation and delivery |
|---|---|
| Available bytes below 10% | Fill/clear only the owned mount; matching firing/resolved alerts |
| Available inodes below 10% | Create/remove bounded tiny files; matching firing/resolved alerts |
| Memory above 90% | Hold/release bounded touched memory in the fixture cgroup; matching alerts |
| Tasks above 90% | Start/stop bounded fixture children; matching alerts |
| CPU throttled periods above 20% | Bounded CPU worker under actual quota, increasing usage/throttle counters and recovery alerts |
| Read-only storage | Actual tmpfs remount, writer `EROFS` denial and matching observer mount flags, then restore and matching alerts |
| OOM kill increase | Actual allocation killed inside the finite cgroup, OOM counter increase and matching alerts |
| Unavailable source | Remove traversal access, omit data families, readyz503/healthz200, then restore and matching alerts |

Every accepted firing/resolved pair matched labels, fingerprint and start time
through the authenticated receiver. These initial runs predate the additional
current-generation boundary described above. Live rule state was checked for
pressure cases. Production
thresholds were unchanged; only rate/pending/transport timing was accelerated.
Logs contain static success reports rather than payloads or credentials; their
workflow timestamps are report times, not individual pressure transition times.

Normal observer stop and a separate OS SIGTERM run passed exact cleanup checks:
owned services stopped, service cgroups empty, native children gone, listeners
closed, tmpfs unmounted and temporary credentials removed. The live session/group
anchor remained until cleanup was verified. The PR run reported normal cleanup
at 16:23:04 UTC and interruption cleanup at 16:23:15 UTC on September 10, 2026.
The push run independently reported both cleanup successes.

## Commands and scope

Local commands completed successfully:

```text
cargo test -p board-resource-monitor --lib --locked
cargo clippy -p board-resource-monitor --all-targets --locked -- -D warnings
cargo test -p board-observe --lib --locked
cargo fmt --all -- --check
cargo +1.94.0 check -p board-resource-monitor --lib --target x86_64-unknown-linux-musl --locked
.local/monitoring/bin/promtool.exe check rules deploy/monitoring/alerts.yml
.local/monitoring/bin/promtool.exe test rules tests/monitoring/resource-rules.test.yml tests/monitoring/rules.test.yml tests/monitoring/queue-rules.test.yml
.local/monitoring/bin/promtool.exe check config --syntax-only deploy/monitoring/prometheus.yml
.local/monitoring/bin/amtool.exe check-config deploy/monitoring/alertmanager.yml
.local/monitor-auth-venv/Scripts/python.exe -m unittest discover -s tests/monitoring -p test_*.py
.local/monitor-auth-venv/Scripts/python.exe -m unittest discover -s tests/monitoring/authenticated -p test_profile.py
git diff --check
```

Git Bash `bash -n scripts/test-resource-monitoring.sh` and Python compilation of
the qualification helpers passed. The delayed-ready-result regression initially
failed, then passed after the elapsed deadline fix. Initial hosted Linux Clippy
failed as described above; those failed runs are not counted as passing evidence.

Linux CI ran `bash scripts/verify.sh` for workspace formatting, all-target/all-feature
Clippy, workspace examples/binaries/tests and browser checks. It then executed
`scripts/test-resource-monitoring.sh` normally and with
`RESOURCE_QUALIFICATION_INTERRUPT=1`, using the exact minimal-environment commands
in [operations](resource-observability.md#owned-qualification). The candidate unit
passed `systemd-analyze verify`; the native fixture separately exercised effective
identities and controls. Production host/path/network/permission equivalence is
not established by that syntax check or by success on the disposable runner.

The final revision must pass its own checks before merge; the PR
rollup is the source for that exact revision's status.

Local WSL remains unresponsive; restart approval is pending. Existing WSL
processes were retained rather than replacing live tests. The full rewrite,
production resource/network/storage qualification and update-check alert delivery
remain incomplete. See [operations and measurement limits](resource-observability.md).
