# Private HTTP and media queue monitoring examples

These examples cover authenticated scrape availability, HTTP 5xx responses,
rejected writes, staff 401/403 responses, and connection-pool pressure. The Rust
exporter records completed handler headers; it does not measure complete body
delivery. Pool samples are process pool estimates, not database health checks.

The separate `board-monitor` process observes aggregate media queue state through
the restricted `board_monitor` database login. Its cached samples also cover queue
capacity, active and expired jobs, oldest queued age, and recent failure counts.
HTTP scrapes and readiness checks only read memory; they never query PostgreSQL.

Example warning thresholds, each sustained for two minutes:

| Alert | Condition |
| --- | --- |
| `BoardHttpDown` | A configured board target has `up == 0` |
| `BoardHttp5xx` | Five-minute 5xx ratio exceeds 5%, with at least 1 response/second |
| `BoardHttpWriteRejections` | Five-minute rejected-write rate exceeds 0.2/second |
| `BoardStaffAuthorizationRejections` | Five-minute staff 401/403 rate exceeds 0.1/second |
| `BoardDbPoolPressure` | `(connections - idle) / max_connections` exceeds 90%, with nonzero maximum |

Low-volume errors may not cross these example rate thresholds. Authorization
counts include CSRF rejections. A stopped scrape target fires `BoardHttpDown`;
removing a target from the scrape configuration does not. Missing metric series
alone do not fire the rate/pool rules. Tune thresholds and routing using staging
load and expected activity before operational use.

Queue warning thresholds are separate project-defined examples:

| Alert | Condition | Hold |
| --- | --- | --- |
| `BoardMediaObserverUnavailable` | Configured observer is down, sample diagnostics are missing, sample success is not 1, or the last success is older than 30 seconds | 30 seconds |
| `BoardMediaQueuePressure` | Receiving + queued + processing jobs exceed 90% of nonzero configured capacity | 1 minute |
| `BoardMediaProcessingFailures` | At least one `processing_failed`, `invalid_output`, or `retry_exhausted` job in the recent window | 30 seconds |
| `BoardMediaIntakeFailures` | At least one `intake_failed` or `abandoned` job in the recent window | 30 seconds |
| `BoardMediaExpiredJobs` | At least one expired receiving, queued, or processing job | 1 minute |

Expired jobs occupy queue capacity until reconciliation changes their state.
Published jobs do not occupy active capacity. Failure metrics are gauges over the
preceding 15 minutes, including the exact lower time boundary; they return to zero
when the failures age out. Do not apply `rate()` or `increase()` to those gauges.
Normal terminal cleanup retains rows for longer than that window. The oldest
queued age is exported for diagnosis without an alert threshold in this example.

The observer starts unavailable, samples immediately and every five seconds, and
makes a failed or more-than-30-second-old snapshot unavailable. In that state only
`board_media_sample_success` and
`board_media_sample_last_success_timestamp_seconds` remain; queue values are
omitted rather than set to zero or left stale. The last-success timestamp starts
at zero and remains after failure. Recovery restores the queue families. Other
HTTP processes do not register a queue callback and do not emit these families.

All five queue rules are restricted to `job="board-monitor"`. The observer rule
uses the configured target's `up` series to detect missing diagnostics even when
all queue families disappear. The four workload rules require a successful,
fresh sample and an up target, so stale or unavailable data cannot keep them
firing. Removing the observer target from Prometheus configuration removes this
anchor and does not alert; keep the required target under configuration review.

Each application uses a separate loopback metrics port and independent
`METRICS_TOKEN` containing exactly 64 lowercase hex characters. Generate tokens
with a cryptographic generator and store the same raw token in the corresponding
`credentials_file`, readable only by the scraper's service account. Set
`METRICS_BIND_ADDR` to the matching `127.0.0.1:9191`, `:9192`, or `:9193` address.
The candidate `board-monitor` target uses `127.0.0.1:9194` and its own independent
token in `/etc/26chan/metrics/monitor.token`. Its database credential is also
independent and is accepted only by `board-monitor`, never the HTTP applications.
Authenticated `/healthz` returns 200; `/readyz` returns 200 only for an available
cached sample and otherwise 503. Health bodies are empty. These routes are enabled
only by the observer's optional health API; existing HTTP exporters retain only
`/metrics`. All routes require the same bearer authentication and no-store headers.
Do not expose these ports through application ingress. A remote scraper requires
an operator-reviewed authenticated encrypted tunnel. The sample paths are Linux
deployment placeholders; qualification creates disposable real credential files.

Bind Prometheus's web listener to `127.0.0.1:9090` and Alertmanager's web listener
to `127.0.0.1:9093`; disable Alertmanager clustering with
`--cluster.listen-address=` for this same-host example. Those processes' default
web/cluster binds are not suitable defaults for a private setup. Supply explicit
configuration and storage paths. Protect their data and APIs, set deployment
connection/write deadlines for these monitoring tools, and manage the processes
with the service supervisor. The Rust metrics exporter itself caps accepted
connections at 16, disables keep-alive, and applies a 10-second total connection
deadline; four retained exposition responses/data allocations may remain admitted.
This repository does not install services or deploy monitoring.

`alertmanager.yml` points to an example **local** webhook on port 9095. A real
operator destination, authentication, on-call routing and receipt evidence still
need deployment work. No external notification is sent by qualification.
The [resource observer](../../docs/resource-observability.md) adds configured
filesystem and local cgroup pressure rules. Its native qualification is tracked
separately. The [maintenance observer](../../docs/maintenance-observability.md)
adds recorded update failure, overdue and unavailable-journal rules. Actual
production update commands/schedules, host qualification and delivery to an
operator receiver remain incomplete.

## Local and CI qualification

From the repository root (Python standard library only):

```sh
python3 -m unittest discover -s tests/monitoring -p 'test_*.py'
python3 scripts/monitoring/download.py
.local/monitoring/bin/promtool check config --syntax-only deploy/monitoring/prometheus.yml
.local/monitoring/bin/promtool check rules deploy/monitoring/alerts.yml
.local/monitoring/bin/promtool test rules tests/monitoring/rules.test.yml
.local/monitoring/bin/promtool test rules tests/monitoring/queue-rules.test.yml
.local/monitoring/bin/amtool check-config deploy/monitoring/alertmanager.yml
cargo +1.94.0 build -p board-observe --example http_fixture --locked
python3 tests/monitoring/qualify.py
python3 tests/monitoring/interruption.py # Linux only
```

On Windows use `python` and add `.exe` to the explicit tool commands. The Python
scripts select the correct binaries automatically. Only x86_64 Linux and Windows
are pinned. The downloader uses official
[Prometheus 3.14.0](https://github.com/prometheus/prometheus/releases/tag/v3.14.0)
and [Alertmanager 0.34.0](https://github.com/prometheus/alertmanager/releases/tag/v0.34.0)
archives, checks fixed SHA256 digests before copying named executable files, and
discards temporary archives. Updates require reviewing and changing the version
and both platform digests in `scripts/monitoring/download.py`.

Promtool's rule tests exercise production thresholds and timing, including an
explicit `ALERTS{alertstate="pending"}` assertion for every rule. Qualification
launches the actual `board-observe` Rust example plus Prometheus and Alertmanager
on dynamically selected loopback ports. It checks unauthenticated rejection,
authenticated exposition with `promtool check metrics`, successful Prometheus
scraping, actual requests that return 500, a firing webhook, healthy requests,
and a resolved webhook with the same alert fingerprint. It checks Prometheus
alert state alongside delivery. The fixture's failure route exists only in the
example binary, not in production applications.

The queue rule file additionally tests unavailable and missing diagnostics,
the 30-second freshness boundary, stale workload suppression, all five fixed
failure reasons, the inclusive 15-minute failure-window boundary and recovery,
unknown state/reason exclusion, and absence of an observer target. These tests
validate PromQL against supplied gauge samples; actual PostgreSQL window and
privilege semantics require the separate queue database qualification described
in [the queue design](../../docs/queue-observability-design.md). The HTTP fixture
has no observer target and does not qualify database-backed queue collection.

Qualification reuses the example configuration and rule expressions, accelerating
scrapes/evaluation to one second, rate windows from five minutes to 30 seconds,
the alert hold from two minutes to four seconds, and notification grouping to one
second. It tests real local transport and recovery, not production timing or an
external receiver. Process readiness/delivery waits have deadlines, subprocesses
are cleaned up on success or failure, diagnostic output is bounded and the test
secret is redacted. The CI workflow is independent of database/browser/media
qualification; those existing checks remain required.

Linux CI also sends OS SIGTERM after a real successful scrape and verifies all
three owned children exit and the temporary credential/storage directory is
removed. Python-delivered SIGTERM unwinds normal cleanup on both platforms.
Windows `TerminateProcess` and OS SIGKILL cannot run Python cleanup handlers;
forced termination requires the runner/supervisor to clean its owned processes
and temporary files. The Linux interruption check runs in its own process group
so a test failure can clean up only that group's remaining processes.

Protocol references:
[exposition format](https://prometheus.io/docs/instrumenting/exposition_formats/),
[rule tests](https://prometheus.io/docs/prometheus/latest/configuration/unit_testing_rules/),
[alert rules](https://prometheus.io/docs/prometheus/latest/configuration/alerting_rules/),
[Alertmanager configuration](https://prometheus.io/docs/alerting/latest/configuration/).
