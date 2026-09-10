# Authenticated monitoring verification

This record covers [issue #38](https://github.com/frankischilling/26chan/issues/38)
and the [authenticated profile](authenticated-monitoring.md), based on main
`19ec11c96181fb0ae5f93983ece822a558e571e7`. It is owned local/CI evidence, not a
production notification integration or approval to launch.

The profile uses the existing pinned Prometheus 3.14.0 and Alertmanager 0.34.0.
Only operator/test Python bcrypt 5.0.0 is added, through hash-pinned binary wheels
in an ignored virtual environment. No Rust dependency, service or migration changes.

## Local qualification

The Windows environment uses Python 3.14.3, Git's OpenSSL 3.5.4, the real Rust
`http_fixture`, and downloaded native monitoring binaries. Native checks of the
unmodified generated profile passed: Prometheus configuration, both server web
policies and Alertmanager configuration. Both native APIs rejected missing/wrong
Basic credentials and accepted their independent operator credentials.

The actual established TLS connection to Alertmanager returned 200 before policy
revocation, 401 after ingest-password replacement, 500 with the policy unavailable,
and 200 after restoration. Socket identity was checked throughout. This verifies
per-request policy reload rather than relying on a new TLS handshake.

The initial full delivery runs did not fire the HTTP alert: synchronous TLS/API
observation limited fixture traffic to 0.38 requests/second, below the unchanged
1/second threshold. Diagnostics showed zero firing alerts and zero send failures.
The harness now drives a bounded independent stream of real Rust requests; the
rule's traffic gate and error threshold were not relaxed.

The corrected actual transport passed on September 10, 2026. A new Prometheus
notification-error count plus its firing state established a rejected ingest send;
an authenticated Alertmanager query showed that alert absent. After ingest
restoration, Alertmanager held the alert and its failed-notification count
increased while the revoked receiver queued nothing. Receiver restoration then
allowed real firing and resolved notifications with fingerprint
`0f54bc37169a24de`, starting at `2026-09-10T07:27:16.73Z` and resolving at
`2026-09-10T07:28:51.73Z`. Prometheus state matched both transitions. The command
exited 0 after owned process/receiver/temp-directory cleanup.

Review corrected two harness gaps before integration: rejection requires an
increase from a per-phase failure-counter baseline, and non-200 observations fail
instead of masquerading as empty results. The Linux watcher retains a live owned
session leader until cleanup completes, including after qualifier failure, and
handles its own SIGTERM. Hosted Linux execution passed as recorded below.

The combined focused suite passed 19 tests in 79.009 seconds, with one POSIX-only
permission test skipped on Windows. Five existing monitoring helper tests and
Python syntax/diff checks also passed. The focused suite exposed a Windows race
when an oversized body continued
uploading while the receiver rejected its declared size and closed the connection.
The bound is retained: an oversized Content-Length must return 413 before body
upload, while a real oversized upload must be rejected without queueing and be
followed by successful authorized recovery. The corrected suite retains these
checks; no unbounded draining is introduced.

Final review also required any unexpected traffic-thread exit to fail the
qualification. Recovery now requires an observed 2xx rate of at least 1/second, so
vanished traffic cannot qualify as recovery by dropping below the alert's traffic
gate. The final Windows transport rerun passed with fingerprint
`f46d086afbb03293`, firing at `2026-09-10T07:34:52.504Z` and resolving at
`2026-09-10T07:36:27.504Z`. It exited 0 with all rejection, recovery, traffic and
cleanup assertions intact.

## Hosted evidence

Implementation commit `ace15637db4c5649ce0d55337e895eb54e85ad45` passed all hosted
checks on September 10, 2026. The subsequent evidence update changes only this
document. [PR #39](https://github.com/frankischilling/26chan/pull/39) records the
final checked head and merge; every current-head check is required before merge.

| Check | Result |
|---|---|
| [PR Linux/PostgreSQL and Windows build](https://github.com/frankischilling/26chan/actions/runs/34450907023) | Both jobs passed, including database permissions/concurrency, browser behavior and the existing media/restore qualifications |
| [Push build](https://github.com/frankischilling/26chan/actions/runs/34450877594) | Both jobs passed independently |
| [PR monitoring](https://github.com/frankischilling/26chan/actions/runs/34450907014) | All 19 focused tests passed on Linux with no skips; native validation, both denied/restored HTTPS links and normal/SIGTERM cleanup passed |
| [Push monitoring](https://github.com/frankischilling/26chan/actions/runs/34450877628) | The same qualification passed independently |
| [Advisory workflow on the implementation head](https://github.com/frankischilling/26chan/actions/runs/34450993655) | Cargo audit checked 327 locked crates against 1,243 advisories; npm audit reported zero vulnerabilities |

The PR's authenticated notification fingerprint was `003bea350b83b68e`, firing
at `2026-09-10T07:39:34.52Z` and resolving at `2026-09-10T07:41:04.52Z`.
At `07:41:12Z`, the OS SIGTERM watcher verified that the three native children,
qualifier, HTTPS receiver listener, private PKI and temporary storage were gone.
It checked actual process-group/session membership before interruption and
retained its owned supervisor until cleanup was acknowledged.

The advisory workflow was explicitly dispatched because its normal PR path
filter covers Rust/npm lockfiles rather than this operator-only Python addition.
The [PyPI release metadata](https://pypi.org/pypi/bcrypt/5.0.0/json), checked on
September 10, reported bcrypt 5.0.0 not yanked and listed no known vulnerabilities.
That is a release-metadata check, not a transitive wheel or OpenSSL audit.

## Limits

Native Basic principals have broad monitoring API access, including Alertmanager
silences; no per-route authority is claimed. The native policy has no password
expiration field. TLS certificate changes apply to new connections, while request
credential revocation is checked on existing connections. An empty native Basic
user map disables authentication and is never an approved generated policy.

The receiver, credentials, certificates, data directories and notifications are
synthetic and owned. Production service identities, Windows ACLs, network policy,
real receiver/account ownership, escalation, durable storage, certificate/secret
operations and deployed resource limits remain unqualified. Local WSL is still
unresponsive; it was not restarted. Linux evidence above comes from the hosted
workflow. Public uploads and production media remain disabled.
