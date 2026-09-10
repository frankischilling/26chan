# HTTP observability implementation plan

> Use superpowers:subagent-driven-development, with scoped reviews and a final
> branch review. User authorization covers implementation and merge after CI.

Goal: authenticated private metrics from the three HTTP applications, with real
local firing and recovery notification evidence.

Spec: [HTTP observability design](http-observability-design.md).
Tech: safe Rust 1.94, existing Axum/Tokio/board-http, subtle constant-time compare,
Prometheus and Alertmanager pinned official binaries, Python standard library.

## Task 1: exporter and fixed metrics

- [ ] Create `crates/observe` and add the workspace member. Implement the exact
  Metrics/Listener/Pool/PoolSample interface in the design. Add callbacks before
  cloning/layering; `Metrics::register_pool(&mut self, Pool, callback)` must reject
  duplicate pools. `Metrics::layer(&self, Router, Listener) -> Router` installs
  the listener exactly once per router. `Metrics::render() -> String` is bounded.
- [ ] Write tests first for `Config::parse(Option<&str>, Option<&str>)`, then
  implement `Config::from_env() -> Result<Option<Config>, ConfigError>` and
  `Endpoint::bind(Option<Config>) -> io::Result<Endpoint>` (async).
- [ ] `Endpoint::serve(self, Metrics, application_future) -> io::Result<()>`
  awaits the application when disabled; otherwise selects application completion
  or exporter failure and drops the paired future. Actual application shutdown
  is handled by its existing graceful shutdown future.
- [ ] Write authenticated HTTP, concurrent accounting, cancellation, fixed-label,
  retained-body, and listener-lifecycle tests, observe red, implement, run
  `cargo test -p board-observe --locked` and warnings-denied clippy.

## Task 2: application integration

- [ ] Add board-observe dependencies to public/staff/media-http. Validate config
  and bind before DB access in each main. Public and API share one Metrics;
  register existing pool snapshot with fixed public label. Staff registers auth
  and staff pools. MediaReader adds an aggregate pool-statistics accessor only.
- [ ] Wrap final application routers through `metrics.layer(app, Listener::...)`
  and pair the server with `Endpoint::serve`. Preserve public connect-info,
  graceful draining, existing body ownership, and media rejection of production.
- [ ] Add real-router composition tests exercising normal health responses,
  missing routes and rejected writes without depending on a live database;
  run relevant packages locally and hosted full database/browser/media CI.

## Task 3: alerts and actual delivery

- [ ] Add `deploy/monitoring/` examples and promtool rule tests covering service
  down, 5xx error rate, rejected writes, staff authorization anomalies, and pool
  exhaustion. Test each rule's healthy, pending, firing and resolved behavior.
- [ ] Add a pinned official tool downloader and Python loopback qualification.
  Launch actual Prometheus and Alertmanager, scrape the actual crate exporter
  example, trigger synthetic 5xx, assert firing webhook, recover, assert resolved.
  Ensure process cleanup on every terminal path, bounded deadlines/output and
  dynamic owned ports. No outside network notification destinations.
- [ ] Run promtool config/rule/exposition checks and qualification; add separate
  Linux CI job with explicit commands/exit checking.

## Task 4: review and merge

- [ ] Review exporter and alert deliverables against the spec, fix findings.
- [ ] Update operations/readiness with exact implemented coverage and remaining
  gaps. Record actual commands, failures, hosted commit/run references.
- [ ] Run fmt, focused tests, workspace clippy/build if local space permits,
  dependency audit, hosted full CI and final branch review. Commit via identity
  helper, push PR and merge only after final-head checks pass; verify main CI.
