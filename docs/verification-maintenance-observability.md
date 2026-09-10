# Maintenance observation verification

Work for [issue #42](https://github.com/frankischilling/26chan/issues/42) starts at
main `5a2ef5c00a2cd8a4be7fef7ddcf59d35026d9251`; design/plan commit `d564998`.
This record describes initial local verification and the first hosted attempt.
[PR #43](https://github.com/frankischilling/26chan/pull/43) records the final
revision, required hosted/native results and merge status. Local checks alone
do not establish Linux behavior or production deployment.

Local Windows checks on September 10, 2026:

- Pinned promtool passed 19 maintenance cases and all existing resource, queue
  and HTTP rule suites. Native validation found 21 rules. The new cases cover
  pending failure across retry, recovery, each overdue cause, unavailable/stale
  sources, target independence and threshold equality. They are rule tests, not
  evidence that actual commands ran.
- Profile tests passed 10 cases with one explicit POSIX-only skip on Windows.
  Native six-job HTTPS configuration validation passed in the implementation
  task. Parent independently reran the profile tests and rule suites.
- Producer regression tests first failed, then passed for strict schemas/state
  transitions and finalization/cleanup behavior. Parent ran 15 tests: seven
  portable tests passed and eight Linux-native tests were explicitly skipped.
  Python compilation, shell syntax, workspace formatting and diff checks passed.
- Rust parser, metrics, cache and native-I/O tests are authored and formatted but
  unrun locally: C: had approximately 494 MB free, WSL remained unresponsive, and
  an earlier automatic approval review rejected old-cache cleanup. No equivalent
  deletion or WSL restart was attempted. Offline Cargo metadata updated only the
  new local package in Cargo.lock; no registry version/checksum changed.

Independent source review accepted the observer's cross-language journal
contract, per-target availability and bounded cache. Recorder review found a
signal window during final publication and an immediate-KILL test cleanup gap;
both were corrected with additional regressions. Finalization now commits a
consistent journal and exit status, and test cleanup retains owned process
identity before forced signaling. Independent scoped re-review accepted both
corrections. Separate integration review accepted the metrics, rules, profile,
runtime, native harness, CI and documentation. Native checks remain separate
from those source reviews and portable results.

Initial implementation `dbfb6b2` passed both monitoring workflows, both Windows
visual jobs and the advisory scan. The
[PR Linux build](https://github.com/frankischilling/26chan/actions/runs/34513086096)
and [push Linux build](https://github.com/frankischilling/26chan/actions/runs/34513080036)
stopped at `clippy::nonminimal_bool` in the maintenance sampler. The missing-or-
stale expression now uses `is_none_or(age > limit)` instead of negating
`is_some_and(age <= limit)`; its boundary and behavior are unchanged. These runs
did not execute the new native maintenance qualification and are not passing
Linux evidence. The PR tracks the corrected revision's checks.

The owned delivery harness starts actual producer commands and a production-mode
observer with a distinct DynamicUser. Its assertions require a real marker
update, nonzero failure and recovery; fresh observation; denied credentials;
journal/config write and operator-payload access denials; overdue, denied-source
and missing-source transitions; current activation and exact firing/resolved
identity through verified HTTPS; and normal/SIGTERM cleanup. These assertions
must pass on Linux before this feature can be merged. The final PR records the
exact revision and run links.

Commands run locally:

```text
.local/monitoring/bin/promtool.exe test rules tests/monitoring/maintenance-rules.test.yml tests/monitoring/resource-rules.test.yml tests/monitoring/queue-rules.test.yml tests/monitoring/rules.test.yml
.local/monitoring/bin/promtool.exe check rules deploy/monitoring/alerts.yml
.local/monitor-auth-venv/Scripts/python.exe -m unittest discover -s tests/monitoring/authenticated -p test_profile.py
.local/monitor-auth-venv/Scripts/python.exe -m unittest discover -s tests/maintenance -p test_*.py
.local/monitor-auth-venv/Scripts/python.exe -m py_compile tests/maintenance/fixture.py tests/maintenance/qualify.py tests/maintenance/interruption.py scripts/maintenance/run.py scripts/maintenance/state.py
bash -n scripts/test-maintenance-monitoring.sh
cargo fmt --all -- --check
cargo metadata --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc
git diff --check
```

No production update command, maintenance schedule, package authenticity result,
rollback policy, deployed privilege boundary or operator destination has been
qualified. The full rewrite remains incomplete; see [operations and limits](maintenance-observability.md).
