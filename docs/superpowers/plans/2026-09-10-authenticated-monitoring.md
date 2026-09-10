# Authenticated Monitoring Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development task by task.

**Goal:** Authenticate both notification links and monitoring APIs with actual delivery evidence.
**Architecture:** Native HTTPS/request auth, strict profile rendering, owned transport qualification.
**Tech Stack:** Existing pinned Prometheus3.14.0/Alertmanager0.34.0/OpenSSL, Python3.12+, bcrypt5.0.0; unchanged Rust.
**Spec:** `docs/superpowers/specs/2026-09-10-authenticated-monitoring-design.md`

## Global constraints

- Current checkout feature branch; no production deployment or external notifications.
- No Rust registry changes; pyca bcrypt5.0.0 is hash-pinned and installed only in an ignored venv.
- The spec's manifest, output names, credential bounds and native authority limits are binding.
- All subprocesses use bounded waits/minimal environments and own their cleanup.
- Reuse existing agents because the thread cannot allocate fresh agent slots; parent owns Git mutations.

### Task 1: Validated profile (profile agent)

Files: `scripts/monitoring/auth_profile.py`, `scripts/monitoring/auth-requirements.txt`,
`tests/monitoring/authenticated/test_profile.py`.
Consumes spec manifest; produces `render(manifest, output)` and four named config paths.

- [ ] Write tests for accepted manifest and typed/endpoint/credential/path/output failures.
  Example: `with self.assertRaises(ValueError): render({**manifest, 'receiver': {**manifest['receiver'], 'url': 'http://localhost/alerts'}}, output)`.
- [ ] Observe missing-renderer failure, implement validation before writes, private exclusive output,
  bcrypt12 hashes and the exact native configuration. Test credential hashes with `bcrypt.checkpw`.
- [ ] Run `.local/monitor-auth-venv/.../python -m unittest discover -s tests/monitoring/authenticated -p test_profile.py`.
  Validate rendered files with real promtool config/web-config and amtool as part of Task3.
- [ ] Source review; parent commits with verified identity after integration.

### Task 2: Owned TLS test support (TLS agent)

Files: `tests/monitoring/authenticated/support.py`, `tests/monitoring/authenticated/test_support.py`.
Consumes existing OpenSSL executable and Python stdlib; produces these APIs:
`make_pki(directory: Path, openssl: str) -> dict[str, Path]`, keys ca, cert, key,
wrong_ca, wrong_cert, wrong_key, expired_cert, expired_key; valid cert has DNS localhost.
`Receiver(cert: Path, key: Path, policy: Path)` is a context manager with `.port`,
`.notifications` Queue, `.url` (https://localhost:port/alerts), and bounded authenticated POST.
`write_policy(path: Path, token: str, *, active=True, not_before=0, expires_at=4102444800)`
atomically writes private hash-based receiver policy. `context(ca: Path)` returns
verified TLS13 SSLContext; `request(url, ca, *, basic=None, bearer=None, data=None)`
returns `(status, bytes)`, raising TLS/network errors. Preserve a caller's original policy path.

- [ ] Write tests proving valid peer/token success and denied missing/wrong/expired/revoked/unavailable policy.
  Example: `assert request(receiver.url, pki['ca'], bearer=token, data=b'{"alerts":[]}')[0] == 200`;
  after `write_policy(policy, token, active=False)`, the same request must return401.
- [ ] Implement bounded OpenSSL generation, receiver and explicit verified HTTP helper; observe rejection
  for wrong CA/hostname/expired certificate against healthy local endpoints.
- [ ] Run focused unittest with configured OpenSSL path; check server thread/socket cleanup.
- [ ] Review and supply APIs to parent; no production receiver claim.

### Task 3: Actual transport, CI and operating procedure (parent)

Files: `tests/monitoring/authenticated/qualify.py`, `tests/monitoring/authenticated/interruption.py`,
`.github/workflows/monitoring.yml`, monitoring/operations/dependencies/readiness/verification docs.
Consumes renderer and support APIs; launches existing Rust fixture and native monitoring binaries.

- [ ] Observe absent qualification failure; build manifest from private synthetic credentials/PKI.
  Native checks: `promtool check config`, `promtool check web-config`, `amtool check-config`.
- [ ] Exercise missing/wrong/revoked/unavailable native Basic policy on existing connection and recovery;
  assert healthy allowed controls. Check authenticated receiver/TLS failures using Task2.
- [ ] Require actual BoardHttp5xx firing/resolved notifications and matching fingerprint through both
  authenticated links, including failed credential delivery suppression and restored delivery.
- [ ] Use an owned lifecycle state and Linux SIGTERM watcher; ensure all child PIDs/storage disappear.
- [ ] CI installs hash-pinned wheel into venv then executes unit/transport/interruption checks.
  Existing full CI remains required. Local Windows executes actual transport if OpenSSL is available.
- [ ] Document per-service secrets/API authority, complete-profile deployment/validation, atomic rotation,
  native expiration/reload limitations and real evidence. Review full branch and correct findings.
- [ ] Commit/push/create PR; merge checked head only after all checks pass; verify main tree/postmerge runs.

## Preflight

Task1 and Task2 have no shared files. Task3 consumes both exact APIs above; parent
keeps CI/docs separate. Each test checks actual behavior rather than file strings.
No production credentials or destination are required for owned qualification;
production integration remains explicitly unverified. The user already authorizes
implementation and passing-check merges; no additional permission gate is needed.
