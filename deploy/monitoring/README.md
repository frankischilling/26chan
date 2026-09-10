# Private HTTP monitoring examples

These examples cover authenticated scrape availability, HTTP 5xx responses,
rejected writes, staff 401/403 responses, and connection-pool pressure. The Rust
exporter records completed handler headers; it does not measure complete body
delivery. Pool samples are process pool estimates, not database health checks.

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

Each application uses a separate loopback metrics port and independent
`METRICS_TOKEN` containing exactly 64 lowercase hex characters. Generate tokens
with a cryptographic generator and store the same raw token in the corresponding
`credentials_file`, readable only by the scraper's service account. Set
`METRICS_BIND_ADDR` to the matching `127.0.0.1:9191`, `:9192`, or `:9193` address.
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
Production media queue/processing, host/database storage and resource pressure,
and update-check freshness also remain outside this slice.

## Local and CI qualification

From the repository root (Python standard library only):

```sh
python3 -m unittest discover -s tests/monitoring -p 'test_*.py'
python3 scripts/monitoring/download.py
.local/monitoring/bin/promtool check config --syntax-only deploy/monitoring/prometheus.yml
.local/monitoring/bin/promtool check rules deploy/monitoring/alerts.yml
.local/monitoring/bin/promtool test rules tests/monitoring/rules.test.yml
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
