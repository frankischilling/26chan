# Queue observability implementation plan

Use superpowers:subagent-driven-development. The user has authorized implementation,
PRs and merge after passing CI. Spec: [queue observer](queue-observability-design.md).
Safe Rust 1.94, existing SQLx/Axum/Tokio, PostgreSQL 16, existing pinned monitoring tools.

1. Database deliverable: migration 0010, board_monitor bootstrap/provisioning,
   typed reader and actual-role aggregate/denial tests. Own migrations, store
   monitoring module/tests, deploy/roles.sql and dedicated DB helper scripts.
   Snapshot fields: capacity, receiving, queued, processing, expired_receiving,
   expired_queued, expired_processing, oldest_queued_seconds, intake_failed,
   abandoned, processing_failed, invalid_output, retry_exhausted (all i64).
   Write owned SQL tests before implementing; test empty/populated/expired/window
   boundaries, invalid/missing policy, revoked grants and no base-table authority.
   `cargo test -p board-store --features database-tests --test monitoring --locked`.
2. Telemetry deliverable: board-observe media callback and optional health routes,
   fixed metric families and alert rules/tests. Preserve existing exporter tests.
   Write unavailable/privacy/duplicate/health tests first, then implementation;
   run cargo test/clippy for board-observe and both promtool rule files. Own
   crates/observe and monitoring rule/config files plus new rule-test YAML.
3. Runtime deliverable: board-monitor, typed config, inherited credential rejection,
   sampler tests and candidate service/operations. Parent owns runtime/config,
   root workspace/lock integration, application child-env filters and docs.
   Test no overlap, failed/stale/initial/recovered states and no SQL in scrapes;
   test startup bind/config rejection before connection and signal shutdown.
4. Qualification: add an owned fixture helper and actual monitor + PostgreSQL +
   Prometheus + Alertmanager exercise. DB helper prepares an isolated owned
   cluster for the Python transport test; parent owns fixture/Python/CI wiring.
   Verify ready/unavailable/recovered, saturation/failure firing and resolution,
   and cleanup. Reuse pinned tool downloader and local-only notification helpers.
5. Review each deliverable and final branch; run fmt, focused tests, workspace
   clippy, dependency audit, hosted full CI and new qualification. Record real
   evidence/limitations. Commit, push, create PR and merge only the checked head;
   verify main tree and postmerge runs. No production deployment.

Local disk space is limited and WSL restart approval remains pending. Use hosted
Linux for real PostgreSQL/native qualification, keeping failures visible; do not
replace it with mocks. Existing unrelated worktrees remain untouched.
