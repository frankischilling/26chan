# Queue observation verification

September 10, 2026. Source `9e874ce8211f5f2f88e35fb083a34d4d48a0cbd2`
is reviewed in [PR #37](https://github.com/frankischilling/26chan/pull/37), addressing
[issue #36](https://github.com/frankischilling/26chan/issues/36). This is owned
local/CI evidence, not production deployment approval.

## Local checks

The following commands passed on Windows:

- `cargo test -p board-config -p board-monitor -p board-observe --locked`
- `cargo test -p board-staff --test config --locked` (with the documented vendored
  OpenSSL Perl path)
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo fmt --all -- --check` and `git diff --check`
- `cargo audit`: 327 dependencies, 1,243 advisories, no finding
- `python -m unittest discover -s tests/monitoring -p 'test_*.py'`: five tests
- Both `promtool test rules` files: existing HTTP rules and 20 queue scenarios
- Native Git Bash syntax checks for the modified provisioning/qualification scripts

The queue fixture built and passed warnings-denied Clippy. Sampler tests cover
initial/failing/stale/invalid/recovered state, skipped ticks, no overlap and dropped
pending work on shutdown. Startup tests prove invalid settings and occupied
exporter sockets fail before database connection. Configuration rejects
non-Unicode inherited observer credentials and ambiguous production connections.
The lockfile adds only the local workspace package; registry versions and checksums
are unchanged.

## Hosted Linux and Windows

The full Rust/database/browser stage passed for this source in
[Build and test](https://github.com/frankischilling/26chan/actions/runs/34445094850).
The actual `board_monitor` login reads the fixed numeric aggregate and cannot read
raw jobs/content/secrets or write the view. Tests exercise missing policy, expiry,
both sides of the failure window, blocked-query deadline, unsafe grants, revoked
access, malformed values and two-row results. Fresh role bootstrap leaves the
observer NOLOGIN until separate provisioning. Windows visual tests also passed.

[Monitoring qualification](https://github.com/frankischilling/26chan/actions/runs/34445094862)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/34445094928)
passed. Monitoring includes both rule suites, real HTTP firing/resolved delivery
and its OS SIGTERM cleanup test.

The same Build and test run passed normal and interrupted queue qualification on
Ubuntu 24.04/PostgreSQL 16. The normal command from
[queue operations](queue-observability.md) completed at 06:37:06 UTC; the
`QUEUE_QUALIFICATION_INTERRUPT=1` run completed at 06:37:13 UTC. The independent
[push run](https://github.com/frankischilling/26chan/actions/runs/34445092349) passed
both modes as well.

Actual evidence:

- Four real reservations filled capacity four and a fifth was rejected. Prometheus
  and Alertmanager delivered pressure firing and resolved notifications after
  those reservations were released.
- A real reserve/queue/claim/fail transition produced the processing-failure alert.
  Aging only the five owned test failures by 16 minutes resolved it through the
  unchanged 15-minute SQL window. The test matched notification labels and
  fingerprints to the owned target and checked Prometheus's firing state.
- Revoking view SELECT caused sample_success=0, readiness=503 and omission of all
  five queue value families from both direct exposition and Prometheus. Regranting
  recovered readiness and fresh values in the same observer process.
- Normal observer SIGTERM exited successfully. The separate interruption watcher
  verified the real qualifier and three monitoring children stayed in the outer
  helper's process group, sent OS SIGTERM to the qualifier, observed exit 143 and
  confirmed those children and their token/storage directory disappeared.
- Both helper modes checked zero remaining jobs, capacity restored to 64 and
  observer SELECT restored before stopping and removing their private PostgreSQL
  clusters. Temporary credentials stayed inside the owned cluster tree.

The complete Linux job also passed media VM/dispatch/service-boundary tests,
legacy migrations and restore. Restore preserved post/asset fingerprints, fifteen
base-table counts, approved-reader and aggregate-observer view access, and denied
protected operations. Candidate `monitor.service` verification passed after the
built executable was installed in the runner's expected path. All PR and push
checks on the recorded source were successful before this evidence-only update.

## Corrections and limits

The first head's Linux run passed database/browser, media boundaries, migration
and restore checks, then failed because the service verifier could not find
`/opt/paperboard/board-monitor`. The current workflow installs the built binary
there before verifying the unchanged candidate unit. No assertion was disabled.
Source review also corrected non-Unicode credential checks, extra aggregate rows,
missing rule/setup wiring and nested process-group cleanup.

Local Ubuntu WSL remains unresponsive after the earlier disk-full failure; it was
not restarted without permission. Actual PostgreSQL and OS tests therefore use
owned hosted Linux runners. The candidate observer unit is syntax/executable
checked, not evidence of its resource limits being enforced on a production host.
No production identities, receiver, downstream authentication or OS limits were
deployed. Storage/resource/update monitoring remains outstanding.
