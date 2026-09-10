# Maintenance observation implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record real maintenance command outcomes and alert through a separate read-only Rust observer.

**Architecture:** Operator-owned atomic journals connect a fixed-command producer
to the observer. Existing private metrics and authenticated monitoring carry
bounded failure, overdue and unavailable observations.

**Tech Stack:** Safe Rust 1.94, existing locked Tokio/Serde/Rustix, Python 3.12+
standard library, pinned native Prometheus/Alertmanager, systemd on owned Linux CI.

**Spec:** docs/superpowers/specs/2026-09-10-maintenance-observability-design.md

## Global constraints

- Preserve full prompt scope; no production updates, deployment, public uploads or private reference reads.
- Target order application, host, media_guest, monitoring; all exact schema/metric names are in the spec.
- Reuse registry versions/checksums; no Cargo builds on low-space local C: without a fresh capacity check.
- Producer and observer must qualify actual commands and identities, not numeric substitutes.
- User authorizes focused branches, commits, pushes and merges after all final checks pass.

### Task 1: Rust observer sources and bounded cache

**Files:** apps/maintenance-monitor/src/{lib,config,journal,sampler,tests}.rs.
Parent owns Cargo.toml and main.rs. Agent owns these five files.

**Interfaces:** Produce `config::Targets`, `config::from_env() -> Result<Targets, ConfigError>`,
`journal::collect(&Targets) -> MaintenanceSample`, `SampleState::new(&Targets)`,
cloneable state, `snapshot() -> MaintenanceSample`, `ready() -> bool`, and
`sample_loop(state, collector, watch::Receiver<bool>)` using the metrics types
defined in the spec and produced by Task 2. Targets exposes configured target
indices to initialize unavailable cache entries before the first collection.

- [ ] Write parser/transition tests before implementation, including this invariant:

```rust
// A failed target must stay unavailable while an independent valid target renders.
assert!(!sample.targets[0].unwrap().available);
assert!(sample.targets[1].unwrap().available);
```

- [ ] Implement strict configuration and bounded descriptor-based journal reads;
  verify wrong target, timestamp, duplicate/unknown fields, ownership, symlink/FIFO,
  missing and recovery cases. No source data in generic errors.
- [ ] Implement one-slot cache admission, late-ready rejection and monotonic stale
  invalidation with controlled async tests mirroring the corrected resource cache.
- [ ] Run focused portable tests/typecheck where capacity permits; otherwise use
  the pushed Linux/Windows CI and record unrun local commands honestly.
- [ ] Request task review, correct findings and include coherent implementation commit.

### Task 2: Fixed metrics, rules and authenticated profile

**Files:** crates/observe/src/{maintenance,maintenance_tests}.rs and lib.rs;
resource target array/tests as required; deploy/monitoring/{alerts,prometheus,alertmanager}.yml;
scripts/monitoring/auth_profile.py; tests/monitoring/maintenance-rules.test.yml;
tests/monitoring/authenticated/test_profile.py.

**Interfaces:** Produce exact MaintenanceSample/MaintenanceTargetSample and
register_maintenance from spec. Add closed job board-maintenance and service
maintenance_observer. Preserve existing metric families and closed ordering.

- [ ] Add failing render tests for available, unavailable, absent targets and
  fractional timestamps; prove no dynamic label or callback duplication.
- [ ] Implement registration/rendering and closed profile changes.
- [ ] Write actual promtool rule cases for every condition/equality boundary:

```yaml
# With sample_success=1 and fresh timestamp, failure_pending=1 must fire;
# retry in progress does not clear it; subsequent producer success does.
```

- [ ] Run pinned promtool and profile tests, then task review and correction.

### Task 3: Operator recorder and native command behavior

**Files:** scripts/maintenance/{run,state}.py; tests/maintenance/test_producer.py.

**Interfaces:** CLI `python3 scripts/maintenance/run.py /absolute/config.json`;
exact config/journal from spec. Zero exit only for successful completed command
and durable final journal. Export pure strict parsing/transition functions from
state.py for portable tests; Linux operation tests skip explicitly on Windows.

- [ ] Write failure-first strict state/config tests and real Linux command cases:

```python
# Invoke the real producer against an owned update fixture.
self.assertEqual(run_update(valid_input).returncode, 0)
self.assertEqual(installed_marker.read_bytes(), expected_version)
self.assertNotEqual(run_update(missing_input).returncode, 0)
self.assertTrue(read_journal()['failure_pending'])
```

- [ ] Implement owner-checked no-follow directory/file I/O, flock exclusion,
  running-before-spawn, atomic fsync publication, monotonic deadline and group
  cleanup with unreaped leader. Null all child streams and clear inherited env.
- [ ] Verify actual failure, timeout, SIGTERM, killed-run restart, concurrent lock,
  immutable malformed prior state and successful retry clearing failure.
- [ ] Run portable tests now and native CI when available; request task review.

### Task 4: Runtime integration, owned delivery qualification and operations

**Files:** workspace/app manifests, apps/maintenance-monitor/src/main.rs;
deploy/maintenance-{monitor,@}.service and timer candidate;
tests/maintenance/{qualify,interruption}.py; scripts/test-maintenance-monitoring.sh;
.github/workflows/{ci,monitoring}.yml; docs/maintenance-observability.md,
docs/verification-maintenance-observability.md and affected operations/architecture/readiness.

- [ ] Integrate observer runtime with existing metrics endpoint, 2-worker/1-blocking
  runtime and bounded shutdown. Required env and readiness follow Task 1.
- [ ] Build owned Linux fixture with distinct observer, protected inputs and actual
  marker update producer. Reuse PKI/profile/receiver helpers and the proven current
  activation matcher; do not mutate established resource qualification behavior.
- [ ] Qualify actual failure/recovery, overdue/recovery and denied-source/recovery
  through the native HTTPS stack. Verify observer write/execution denials with
  positive operator controls, bad scrape credentials and exact pair identities.
- [ ] Verify normal/SIGTERM exact cleanup, add CI commands and candidate unit checks.
- [ ] Record commands, real failures/unrun checks, deployed limits and remaining
  full rewrite requirements; independently review whole branch.
- [ ] Commit/push draft PR linked to focused issue, wait for all final checks,
  merge exact checked head and verify main tree/clean status and post-merge jobs.
