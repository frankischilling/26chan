# Private HTTP metrics and alert delivery

This slice implements the request and database-pool portion of prompt section 11.
It does not close production observability: media queue/processing, host/database
storage and resource pressure, update-check freshness, and a real operator
notification destination still require separate work and deployment evidence.

Use a shared safe Rust `board-observe` crate with fixed atomic counters and a
Prometheus text 0.0.4 exporter. This small fixed schema avoids a global mutable
registry and user-controlled labels; a general metrics SDK is unnecessary for
these bounded counters. Logs alone cannot supply reliable scrape and alert state.
An external proxy exporter would miss internal pool pressure and cancellations.

Each public, staff and media HTTP process optionally binds a separate loopback
listener using `METRICS_BIND_ADDR` and `METRICS_TOKEN`. Both absent disables it;
partial, non-Unicode, wildcard/non-loopback, zero port, or invalid token settings
fail startup. Tokens are exactly 64 lowercase hex characters from an operator's
cryptographic generator. Every scrape requires exactly one Authorization header
with `Bearer <token>` and a constant-time equal-length comparison. No token is
logged or included in errors. Never route this port through public/staff/media
ingress. Same-host access still requires authentication; remote scrapers need an
operator-reviewed authenticated encrypted tunnel. No production deployment here.

Acquire the metrics socket before connecting databases. Failure to bind prevents
the application serving. The metrics server stops when application serving stops,
and a metrics server error stops its paired application. GET /metrics only, with
Axum's normal HEAD behavior, no CORS, no-store and nosniff headers. At most four
export response bodies/data allocations remain admitted, using board-http's
retained-body helper. Scrapes perform bounded synchronous atomic reads/callbacks,
not SQL or filesystem/network operations. Underlying HTTP connection/write
deadlines remain an operator deployment requirement.

`Metrics` is a clonable process-owned Arc. `Metrics::layer(Router, Listener)`
wraps the outermost application response layer, counts final status and handler
duration, and uses a drop guard for cancellation/inflight recovery. It does not
replace response bodies or extensions. Listener values are public, api, staff,
media. Status classes are 1xx through 5xx and other. Methods are not labels.
There are no request-derived label strings. Fixed counters:

- `board_http_responses_total{listener,status_class}`
- `board_http_write_rejections_total{listener}` (POST/PUT/PATCH/DELETE >=400)
- `board_http_authorization_rejections_total{listener}` (401/403; includes CSRF)
- `board_http_capacity_responses_total{listener}` (429; 503 is covered by 5xx)
- `board_http_cancelled_total{listener}`
- `board_http_handlers_inflight{listener}`
- `board_http_handler_duration_seconds_{bucket,sum,count}{listener}` with
  cumulative 0.01, 0.1, 1, 5, 10, +Inf buckets; completed handlers only.

Handler duration/inflight end when headers return, not when bytes reach clients.
Only installed listeners appear. Counters reset on process restart. A scrape is
an approximate concurrent snapshot. Pool callbacks use a closed Pool enum
(public, staff_auth, staff_content, media_read), returning
`PoolSample { size: u32, idle: usize, max: u32 }` without exposing credentials.
Expose `board_db_pool_{connections,idle,max_connections}{pool}`. These are pool
pressure estimates, not database health or server connection/storage capacity.

Provide Prometheus scrape and alert configuration, rule unit tests with healthy,
pending, firing, and recovery cases, plus an owned loopback qualification that
starts actual pinned Prometheus and Alertmanager binaries and verifies scrape ->
rule -> firing webhook -> resolved webhook. Test credentials and webhook content
are synthetic and local; no external notification is sent. Pin official release
downloads by SHA256. CI must run qualification and check exposition syntax.
Example thresholds are project-defined and need staging/load tuning.

Tests must cover config rejection/redaction, missing/wrong/duplicate auth,
HEAD/cache behavior, no public metrics route, label cardinality/privacy,
concurrent counters, cancellation and retained response admission, pool snapshot,
listener bind/shutdown, and real application wrapper composition. The existing
database/browser/media suite remains required in hosted CI; local WSL is currently
unresponsive and restart approval is pending.

Protocol references: [text exposition](https://prometheus.io/docs/instrumenting/exposition_formats/),
[alert rules](https://prometheus.io/docs/prometheus/latest/configuration/alerting_rules/),
[Alertmanager configuration](https://prometheus.io/docs/alerting/latest/configuration/).
