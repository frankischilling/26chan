# Resource Observability Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development task by task.

**Goal:** Measure configured storage and service pressure and qualify real alerts.
**Architecture:** Separate unprivileged Linux observer, cached fixed metrics,
actual bounded mount/cgroup qualification through authenticated notification links.
**Tech Stack:** Existing Rust1.94/Tokio/Serde/board-observe, locked Rustix1.1.4;
existing Python/OpenSSL/Prometheus3.14.0/Alertmanager0.34.0 qualification tools.
**Spec:** `docs/superpowers/specs/2026-09-10-resource-observability-design.md`

## Global constraints

- Current checkout feature branch; no production deployment or public upload enablement.
- No registry version/checksum changes, no first-party unsafe Rust, no database dependency.
- Exact labels, bounds, metrics, settings and native limits are defined by the spec.
- Parent owns workspace/app manifests, runtime/cache, docs, CI, Git and GitHub mutations.
- Reuse existing agents because no fresh slots are available; narrow file ownership.

## Task 1: Strict configuration and native collection

Own `apps/resource-monitor/src/config.rs`, `collect.rs`, and corresponding unit
test files. Parent creates Cargo/lib scaffold. Consume board-observe ResourceSample,
StorageSample, ServiceSample, STORAGE_TARGETS and SERVICE_TARGETS exactly as spec.
Produce:

```text
config::Targets: Clone+Debug with pub storages:[Option<PathBuf>;4],
                                  pub services:[Option<PathBuf>;10]
config::ConfigError: static Display+Error
config::from_env()->Result<Targets,ConfigError>
config::read_config(&Path)->Result<Targets,ConfigError>
config::parse_config(&[u8])->Result<Targets,ConfigError>
collect::CollectError: static Display+Error
collect::collect(&Targets)->Result<ResourceSample,CollectError>
```

- [ ] Write red tests for strict config, canonical paths, duplicate targets/fields,
  source bounds, finite ceiling parsing, overflows and missing/duplicate stat keys.
- [ ] Implement Linux O_PATH/fstatvfs and checked cgroup-relative reads; unsupported
  OS returns a static collection error. Pure parsing/property tests run on Windows.
- [ ] Observe actual Linux statistics in CI; no fabricated filesystem claims.
- [ ] Run focused tests, source review and correction; parent commits integration.

## Task 2: Exposition, rules and profile integration

Own `crates/observe/src/resources.rs`, `resource_tests.rs`, targeted lib.rs edits;
the new resource group in `deploy/monitoring/alerts.yml`, `tests/monitoring/resource-rules.test.yml`;
targeted auth_profile.py/test_profile.py and plain Prometheus/Alertmanager examples.
Consume exact sample structures; produce register_resources and fixed families.

- [ ] Add red exposition tests for all target slots, absent/unavailable data,
  counter/gauge types, unit conversions, unique registration and label bounds.
- [ ] Implement bounded memory-only formatting; no filesystem work on scrape.
- [ ] Implement eight specified rules and meaningful transition/denominator/reset tests.
- [ ] Extend both profiles to board-resource, independent fifth credential and
  storage/service grouping. Append resource rules to the existing alerts.yml,
  preserving the manifest's single explicitly validated rules_file contract.
- [ ] Native config/rule verification and existing observer regression tests.

## Task 3: Real Linux pressure and service qualification

Own `scripts/test-resource-monitoring.sh`, `tests/monitoring/resource_fixture.py`,
`resource_qualify.py`, `resource_interruption.py` as needed; parent owns CI wiring.
Consume observer binary/settings, exact sample families and profile/PKI APIs.

- [ ] Begin with absent qualification failure, then implement exact owned tempfs,
  private state and limited transient service lifecycle. Minimal environments,
  bounded commands, distinct observer UID, no unrelated service/process cleanup.
- [ ] Exercise healthy/denied scrapes, real bytes/inodes/memory/tasks/CPU pressure
  and recovery, firing/resolved notifications over both authenticated links.
- [ ] Exercise real read-only/OOM/unavailable-source observations, healthy controls,
  denied payload/control access, and normal/OS SIGTERM cleanup of all owned resources.
- [ ] Syntax/helper tests locally; real native execution on Linux CI while WSL remains
  unavailable. Report actual assertion evidence and limits, not config string checks.

## Task 4: Parent runtime, integration and delivery

Own Cargo workspace/lock, `apps/resource-monitor/Cargo.toml`, `src/lib.rs`,
`src/main.rs`, cache/runtime tests, `deploy/resource-monitor.service`, CI and docs.

- [ ] Create minimal crate scaffold, then red tests for sample cache freshness,
  failures, timeout without overlapping blocking collection and bounded shutdown.
- [ ] Implement runtime with two async workers/one blocking collection, five-second
  sample interval/two-second deadline/30-second cache age and authenticated health.
- [ ] Wire resource rule verification and Linux qualification into appropriate CI.
- [ ] Update operations/dependency/readiness/ASVS evidence where supported; document
  filesystem/local-ceiling semantics, startup failure and remaining host/global gaps.
- [ ] Integrate task results, review full branch, correct findings, run required checks.
- [ ] Commit/push/open PR; merge exact checked head after passing CI, verify main tree.

## Preflight rulings

Tasks 1/2 share only declared sample APIs; tasks 3/4 consume configuration and native
metrics. Parent controls manifests and avoids concurrent Cargo mutations. Task2
extends the existing explicitly supplied rule file, adding no hidden dependency.
The user has authorized the
full implementation and passing-check merges; no repeated approval gate is needed.
