# HTTP metrics and alerts

The three HTTP binaries can expose process-local Prometheus metrics on a separate,
authenticated loopback listener. Public and API listeners share one exporter and
public pool; staff has separate authentication and moderation pool series; media
serving has an approved-reader pool series. No new database login or grant is
required. The development media reader still rejects production startup.

For each service, put both settings into its existing operator-owned environment
file. Choose distinct ports, for example public 9191, staff 9192, media 9193:

```text
METRICS_BIND_ADDR=127.0.0.1:9191
METRICS_TOKEN=<independently generated 64 lowercase hexadecimal characters>
```

Generate 32 random bytes with a cryptographic generator and hex-encode them. The
placeholder above is deliberately invalid. Do not reuse a staff, database, media
dispatch, or another exporter's credential. Copy only the corresponding bearer
token into the scraper's protected credential file. Keep tokens out of command
arguments, source control, issue text and logs. Restart the service after rotation
and update the scraper's credential file through the operator channel.

Both variables absent disables the exporter. Partial/invalid/non-Unicode settings
fail startup; so does an occupied metrics port. Binding happens before database
connection. Only nonzero loopback socket addresses are accepted. The deployment
proxy must never forward this port from public, API, staff or media ingress. Remote
scrapes require a reviewed encrypted and authenticated transport to this local
endpoint; loopback bearer HTTP alone is not a remote-network deployment.

`GET /metrics` requires exactly one `Authorization: Bearer <token>` header.
`HEAD` uses the same authorization with no response body. Responses use no-store
and nosniff; no CORS policy authorizes browsers to read them. Four retained scrape
responses/data allocations may be admitted at once. Authentication and exposition
do not query databases. The exporter uses HTTP/1 with at most 16 accepted
connection tasks, no keep-alive, and a 10-second total connection deadline,
including request headers and response writes. Pending OS accepts are outside
that task count. Public/staff/media transport deadlines and deployed host resource
limits still need separate qualification.

| Series | Meaning |
|---|---|
| `board_http_responses_total` | Completed handlers by fixed listener and status class, including middleware rejection and health routes |
| `board_http_write_rejections_total` | POST, PUT, PATCH or DELETE responses with status at least 400; includes validation, policy, authorization and storage errors |
| `board_http_authorization_rejections_total` | 401/403 responses; includes absent sessions and CSRF, not a count of confirmed attacks or distinct people |
| `board_http_capacity_responses_total` | 429 responses; 503 overload/storage failures appear in the 5xx response count |
| `board_http_cancelled_total` | Handler future dropped before a response was produced |
| `board_http_handlers_inflight` | Handler futures currently active; excludes response-body transmission |
| `board_http_handler_duration_seconds` | Histogram for completed handlers with 0.01, 0.1, 1, 5, 10 and +Inf second buckets |
| `board_db_pool_connections`, `board_db_pool_idle`, `board_db_pool_max_connections` | Approximate local SQLx pool occupancy and configured maximum; no SQL probe |

Labels are fixed enums: listeners public/api/staff/media, status classes, and pools
public/staff_auth/staff_content/media_read. No URL, query, IP, cookie, post, account,
board identifier, SQL error or credential enters a label. Counters reset when the
process restarts. Concurrent scrapes are approximate snapshots; a pool gauge is
neither database readiness nor server-wide capacity. Handler latency ends when
response headers return, and says nothing about client receipt or retained body
memory. Request admission still follows [response ownership](verification-response-admission.md).

The candidate configurations and rule tests live in `deploy/monitoring/` and
`tests/monitoring/`. The qualification under `scripts/monitoring/` uses actual
Prometheus and Alertmanager binaries and sends only synthetic alerts to an owned
loopback webhook. It is not evidence that an operator received a production page.
The configured rule thresholds are project choices and require staging/load
tuning, notification ownership and escalation policy before launch.
These local examples do not authenticate the Prometheus-to-Alertmanager or
Alertmanager-to-webhook hops. Both need independently authenticated transport
and restricted ingress before any deployment; their loopback placement in the
synthetic test is not an authenticated service boundary. Protect the monitoring
tools' own APIs and dashboards too.

This slice covers scrape availability, HTTP errors/rejected writes, staff 401/403
rates, and pool pressure. The separate [aggregate queue observer](queue-observability.md)
covers media queue pressure, expiry and processing failures. Neither covers
host/database disk or cgroup resource pressure, update-check freshness/failure,
external uptime, durable monitoring storage, or a production receiver. Those
remain launch prerequisites. Avoid interpreting a healthy scraper as a healthy
application: use readiness and representative external requests too.

References: [Prometheus exposition](https://prometheus.io/docs/instrumenting/exposition_formats/),
[alert rule lifecycle](https://prometheus.io/docs/prometheus/latest/configuration/alerting_rules/),
[Alertmanager configuration](https://prometheus.io/docs/alerting/latest/configuration/).
