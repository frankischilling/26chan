# Windows receive/connect diagnostic

This experiment compares serialized HTTP traffic with sibling connection starts during an incomplete response. It investigates the Windows browser failures that report Winsock error 10055. A successful run does not explain those failures or establish that the application is free of resource problems.

The workflow runs four fresh Windows 2025 jobs: serialized and overlap schedules, each with per-socket `SO_RANDOMIZE_PORT` explicitly disabled or enabled and read back. It does not change machine-wide networking settings. The existing browser checks remain required.

## Workload

Each arm uses the owned visual fixture on IPv4 loopback port 3000. It creates 20 pools of six sockets, runs six HTTP exchanges per socket, closes the pool, waits before the next pool, and finishes with one new socket and one readiness request. A complete arm has 121 connection starts and 721 completed HTTP responses, with at most six live sockets. There are no connection retries or replacement sockets.

Lane zero's first response contains 1,024 prefix bytes and 3,072 suffix bytes. The fixture holds the suffix for at least 250 ms after polling out the prefix. In the serialized arm, all six connections finish before HTTP exchanges run one at a time. In the overlap arm, the client consumes the incomplete prefix before starting the other five connections and services lane zero between those starts. Later overlap rounds service all six sockets; serialized rounds remain sequential.

The validator checks each socket's setup, option readback, partial writes, receives, exchange order, pool boundaries, and cleanup. In overlap arms, it requires all five sibling connect calls to return less than 200 recorded milliseconds after the first pre-`WSASend` request marker. It also requires a matching fixture trace with the full suffix delay. This causal timing check uses durations within each process; it does not compare their clock origins. A delayed arm that misses the exposure window is inconclusive.

The fixture records HTTP body polls. Those records do not prove TCP delivery. The native client uses nonblocking `recv`; this experiment does not establish that a kernel receive operation was pending when a connection started.

## Bounds and ownership

- 30 seconds for work, 35 seconds total elapsed for native cleanup, and a 40-second process watchdog
- One-second connection and HTTP operation deadlines
- 8,192 bytes per response and 30,000 operation records
- 8 MiB native evidence and 512 KiB fixture evidence limits in the validator
- At most 128 fixture records; the normal complete fixture emits 80

The launcher starts the fixture and probe in owned Windows Job Objects. It verifies the listener's process, executable, and start time before and after the probe. Only that owned process tree is terminated during cleanup. A pending overlapped write retains its buffer, `OVERLAPPED`, and event storage; an unresolved cancellation makes the arm unqualified and keeps the storage until process exit.

The fixture endpoint is enabled only on Windows with both diagnostic gates set by the launcher. Each of its 20 pool identifiers can be claimed once. It accepts no arbitrary URL, delay, or body size.

## Evidence and interpretation

See [SCHEMA.md](SCHEMA.md) for record shapes and outcome meanings. The hosted artifact includes candidate and runner identity, hashes, bounded native and fixture records, validation, and launcher status. It keeps only allowlisted compiler diagnostic labels, numeric locations, severity, and diagnostic codes. Raw compiler prose, private fixture logs, and the compiler environment are not uploaded. Artifact retention is three days.

All four arms must complete and qualify before this experiment is described as passing. An actual 10055 is retained as an observed native error, not converted into success. Missing, unreadable, or corrupt fixture evidence is reported separately and prevents qualification without erasing a validated native failure. Malformed native evidence is still rejected. Other transport failures, harness failures, malformed evidence, and missed exposure remain separate outcomes. Passing these bounded arms is not a browser reproduction and cannot justify merging a PR whose full CI is pending, cancelled, or failing.

## Local checks

The response parser tests are portable C++17. The evidence controls run with `node --test tests/fixtures/windows-receive-connect-probe/validate-output.test.mjs`. Rust fixture tests run with `cargo test -p board-public --test visual_fixture_transport_overlap`.

Those checks can run on Linux. Native Winsock compilation, PowerShell execution, Job Object behavior, and the four live arms still require Windows. Synthetic evidence tests exercise rejection and accounting rules; they are not live transport results.
