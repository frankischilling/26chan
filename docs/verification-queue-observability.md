# Queue observation verification

Implementation under review on September 10, 2026. Hosted PostgreSQL and actual
notification qualification are pending; this record is not a deployment approval.

Local Windows checks so far: sampler startup/failure/stale/recovery, skipped ticks,
single-query cancellation/drop and startup-before-database rejection tests pass.
Configuration tests reject non-Unicode inherited observer credentials, ambiguous
production connection settings and encoded socket hosts. The 24 exporter tests,
warnings-denied exporter Clippy, 20 queue rule scenarios, existing HTTP rule tests
and monitoring configuration checks pass. The owned queue fixture compiles and
passes warnings-denied Clippy; all five monitoring Python helper tests pass.
The full workspace passes all-target/all-feature warnings-denied Clippy; focused
configuration, observer, exporter and staff-startup tests pass. Cargo-audit scanned
327 dependencies against 1,243 advisories without a finding. Both rule suites and
the modified shell scripts' syntax checks pass. The lockfile changes only by adding
the local workspace package.

Actual PostgreSQL role/view/window tests, restoration with the aggregate observer,
and observer-to-Prometheus-to-Alertmanager firing/resolved/recovery are wired into
Linux CI and must pass before merge. An additional real SIGTERM watcher verifies
child-process and credential cleanup; the helper checks restored queue state and
grants. Its OS execution is also pending. Local Ubuntu WSL remains unresponsive after
the earlier disk-full failure; no restart permission has been received. Local
compile or rule-unit success is not a substitute for that database evidence.

No production identities, receiver, downstream authentication or OS limits were
deployed. Storage/resource/update monitoring remains outstanding.
