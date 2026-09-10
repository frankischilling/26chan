# HTTP observability verification

Scope: [private HTTP/pool metrics](http-observability.md), tracked in
[issue #34](https://github.com/frankischilling/26chan/issues/34), branch
`feature/http-observability`, based on main
`d11d56a4773d5bc13180715cb7a9dfd3a794a3c7`.
The base archive postmerge run
[34438790780](https://github.com/frankischilling/26chan/actions/runs/34438790780)
passed both Linux and Windows jobs before this change.

## Local results, September 10, 2026

| Command | Observed result |
|---|---|
| `cargo test -p board-observe --locked` | 18 tests passed, including actual socket lifecycle, authentication, connection/response limits, deadline, cancellation and concurrent histogram invariants |
| `cargo test -p board-public -p board-staff -p board-media-http --tests --locked` | All selected default-feature tests passed, including new real-router composition and all three binaries' invalid/occupied metrics startup tests; database-feature tests were not enabled |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed for the complete workspace, including database-feature test compilation |
| `cargo audit` | Passed: 326 locked crate dependencies, 1,243 advisories; no registry package versions changed |
| `python -m unittest discover -s tests/monitoring -p 'test_*.py'` | Two tests passed, including intact/changed download checks and Python signal-handler cleanup |
| `.local/monitoring/bin/promtool.exe test rules tests/monitoring/rules.test.yml` | Passed: all five alert lifecycle tests and low-volume, non-staff and zero-pool-maximum negative cases |
| `cargo build -p board-observe --example http_fixture --locked` | Passed |
| `python tests/monitoring/qualify.py` | Actual native Windows Prometheus/Alertmanager qualification passed; config/exposition checks, exact 401 missing/wrong credentials, authenticated scrape, healthy state, firing webhook and matching recovery webhook |

The Windows staff build used the existing ignored portable Perl prerequisite:
`OPENSSL_SRC_PERL=C:/Users/imike/4chan-rewrite/.local/strawberry-perl/perl/bin/perl.exe`.
The tool downloader verified official Prometheus 3.14.0 and Alertmanager 0.34.0
SHA256 digests. Linux and Windows pins also matched their official checksum files.
The qualification's first native delivery pair had fingerprint
`0eed96db26450faf`, firing at `2026-09-10T05:15:40.122Z` and resolved at
`2026-09-10T05:16:09.122Z`. Its three child processes exited and its owned
temporary credential/storage directory was removed.
Replay with the final connection-limited exporter also passed: fingerprint
`bfaef76e3e8f746f`, firing `2026-09-10T05:20:56.063Z`, resolved
`2026-09-10T05:21:26.063Z`; the three processes and owned temporary directory
were again absent afterward.

## Failures that drove changes

- The public composition test initially failed because the observed router did
  not exist. A later assertion incorrectly expected wildcard CORS without a
  request Origin; the test was corrected to exercise the existing exact-origin
  policy, with no policy change.
- Concurrent histogram reads exposed a final bucket of 1 with count 2. Disjoint
  atomic bins now produce cumulative buckets and reuse their final sum as count.
- Socket tests initially admitted a seventeenth connection, left incomplete
  headers open past the intended deadline and preserved keep-alive. The exporter
  now caps owned accepted tasks at 16, disables keep-alive and applies a ten-second
  total deadline. Three real-socket tests pass.
- Review found that OS SIGTERM would skip Python cleanup. The qualifier now
  unwinds through its cleanup stack; a dedicated Linux CI exercise interrupts
  actual Prometheus/Alertmanager/exporter processes and checks their removal and
  deletion of temporary credentials. Successful cleanup skips the fallback group
  signal after its parent has already been reaped.

Scoped source reviews covered the exporter, application integration, rules,
download verification and notification lifecycle. Requested coverage for a
disabled API listener and media startup was added. The final connection-limit
review accepted its ownership, deadlines and documentation.

## Required hosted checks and limits

The PR must pass the final-head full Rust/PostgreSQL/browser/native-media and
Windows visual workflow, dependency advisories, and the new independent HTTP
monitoring workflow before merge. The monitoring workflow additionally runs
`python3 tests/monitoring/interruption.py` with actual Linux OS-delivered SIGTERM.
The PR checks are the source of truth for those hosted outcomes; local Windows
signal-handler tests do not substitute for Linux process interruption.

Local Ubuntu remains unresponsive following the earlier archive verification's
disk-full/linker and database-timeout failures. Existing WSL handles are retained;
no distro restart was performed while approval was pending. Local database,
native Linux containment, and Linux interruption qualification were therefore
not rerun for this slice. Full workspace binary/example linking was left to hosted
CI after focused Windows builds reduced free disk space. Workspace clippy did
complete locally.

No monitoring service was deployed and no external notification was sent. Tests
use an owned loopback receiver and synthetic requests, accelerate rule windows
and grouping for transport qualification, and retain production thresholds in
promtool unit tests. The authenticated boundary exercised here is application
exporter to scraper. Authentication for monitoring-tool APIs and downstream hops,
an actual operator receiver, queue/processing, storage/resource/update metrics,
and production load/transport/deployment evidence remain open.
