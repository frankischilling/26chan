# Bounded native Windows transport probe (opt-in, unqualified)

This standalone diagnostic reproduces selected Chromium 151 Windows unbound IPv4 TCP connect/close calls against the existing project-owned visual fixture. It does not fix or suppress the unresolved Winsock 10055 failure. No required CI configuration, browser flags, application code, registry, port range, firewall or network settings are changed. There is no retry or fallback to a passing result.

## What is already established

PR #229 head `30b908912fb01071a4609ff627ef9b621d807361`, [run 37750206375 job 113221300567](https://github.com/frankischilling/26chan/actions/runs/37750206375/job/113221300567), finished with 147 passing cases and one navigation failure. NetLog request 10722 reaches socket 10729 directly at the fixture's loopback listener; the retained positive stderr marker identifies an immediate `connect()` 10055. NetLog alone cannot distinguish that branch from an asynchronous FD_CONNECT error. The job used Windows Server 2025 build 26100, image 20260925.250.1 and Chromium 151.0.7922.34. The resource samples do not establish exhausted ephemeral ports or a server socket leak.

Pinned-source defaults enable `TcpPortRandomizationWin` with minimum `WIN11_22H2`/build 22621. Chromium maps build 26100 to a version above that gate. Thus an attempted option call is expected absent a feature override. This is not proof of the actual runtime feature state or successful option application: Chromium ignores `setsockopt`'s return. The retained NetLog has empty `clientInfo` and `activeFieldTrialGroups`, and supplies no option result.

## Model and deliberate limits

The probe uses `WSASocket(AF_INET, SOCK_STREAM, IPPROTO_TCP, WSA_FLAG_OVERLAPPED)`, `ioctlsocket(FIONBIO)`, `TCP_NODELAY`, the 45-second idle/interval `SIO_KEEPALIVE_VALS` defaults, two event handles corresponding to the default Chromium core, `WSAEventSelect(FD_CONNECT)`, then unbound `connect`. A pending connection uses `WSAEnumNetworkEvents` and the FD_CONNECT error. Socket release calls `shutdown(SD_SEND)`, `closesocket`, and closes both event handles. It does not implement Chromium's optional explicit-bind path or IOCP read/write core.

Variants explicitly set and read back `SO_RANDOMIZE_PORT=FALSE` (`plain`) or `TRUE` (`randomized`). This is a controlled native contrast, not an assertion that Chromium ran with either value. Chromium feature-off skips that setsockopt call; explicit FALSE is a deliberate difference. The probe treats rejected/unsupported options as failed qualification and stops that socket, whereas Chromium ignores some option failures. The extra getsockopt/getsockname observations also differ. The harness accepts a synchronous connect success; the pinned Chromium branch marks that outcome NOTREACHED. It never represents such a result as equivalent browser behavior. A returned Boolean is verified before any connection is interpreted as belonging to a variant.

After connect, each socket issues one identical GET `/readyz` with keep-alive and verifies the exact synthetic fixture body. This is a minimal server-health control, not a recreation of browser HTTP pools, contexts, cache, sandbox, task scheduling, screenshots or Chromium's overlapped writes/read-event implementation. Post-connect I/O uses nonblocking send/recv with bounded select waits. A partial send continues only the unsent suffix; no connection or HTTP request is retried. Distinct preplanned attempts continue after failures, and every failure makes the run nonzero.

## Bounds and privacy

- Default 64 batches, 6 connections per batch:384 attempts. Maximum 128 batches:768 attempts.
- At most 6 sockets and 12 event handles held concurrently; no threads or extra processes inside the native probe.
- Batch due times are fixed 50 ms offsets from start. Overdue batches start immediately, with actual event timestamps recorded; there is no adaptive backoff. Response-dependent delays mean these are identical intended schedules, not a claim of identical realized timing.
- Each socket gets an absolute 1 second connect deadline from initiation; late completion observations fail the deadline even if an event has become signaled; no new wait budget is granted. Each HTTP exchange is bounded by 1 second, within a 30 second overall application budget. The launcher imposes a 40 second hard deadline and terminates only its exact child if necessary. The useful-work budget is 30 seconds; failed/overdue runs may emit cleanup evidence until the 40-second hard limit. No result with total elapsed time over 30 seconds can pass. The wrapper allows up to 5 additional seconds to reap an exact killed child. Hard termination is incomplete evidence, never a pass.
- HTTP response cap 8192 bytes; at most 20000 operation records, plus bounded failure/summary records; validator enforces 4 MiB. If collection truncates, evidence is incomplete and cannot pass. Cleanup still executes on normal control-flow exits and through per-socket RAII unwinding.
- Output contains fixed stage names, numeric results/errors, elapsed times, the actual bounded option Boolean/readback length, assigned-local-port Boolean state and aggregate ownership counts. No addresses, actual port numbers, PIDs, request/response contents, secrets, browser profile data or raw kernel events are exported.
- Each API error is captured immediately before another Winsock call. Operation errors such as initial unbound getsockname or shutdown of a rejected connection remain visible even when expected; failure rows identify qualification failures separately.
- Nonzero status, missing/incomplete summary, failed variant readback, any 10055 or unreleased ownership forbids a passing result. The validator is fail-closed; it never repairs evidence.

## Opt-in execution on Windows

The dedicated Windows native transport diagnostic workflow runs for pull requests changing this harness or its workflow, and supports manual dispatch. It uses two fresh Windows runners, one per variant. Compare the recorded checkout, fixture binary hash, OS/image version, batch count and toolchain before treating their exposure as matched. Running both variants sequentially on one machine introduces TIME_WAIT/order contamination and is not the preferred comparison. No repeated runs are automatically scheduled.

1. Use the repository's pinned Rust and Unicode-runtime setup; build the existing example with `cargo build -p board-public --example visual-fixtures --locked` in the specified checkout. Use the same exact source revision for both variants.
2. Start that checkout's `target/debug/examples/visual-fixtures.exe` as an explicitly owned fixture process. Its public listener must be `127.0.0.1:3000`. Do not start Playwright or other browser suites concurrently.
3. In PowerShell 7 with an already-installed official Visual Studio C++ developer shell, run this directory's `run-probe.ps1 -Mode plain -FixtureRepo <absolute-checkout> -Batches 64`. On the separate matched runner use `-Mode randomized`. The launcher verifies the listener belongs to the exact expected fixture executable before compilation and again immediately before launch. It does not authenticate the server throughout the run; a later ownership change remains a limitation. It compiles locally using the installed `cl.exe`; it installs nothing.
4. Each invocation writes a new `probe-<mode>-<random-id>` directory containing the executable, compiler intermediates, early metadata.json, status.json, output.jsonl, compiler.stdout.txt, compiler.stderr.txt, native.stderr.txt and validator stdout/stderr. The status retains compiler/native/validator exit codes, timeouts and child-cleanup completeness, including failed preparation or compilation. Existing outputs are not overwritten. The metadata records OS version/build and fixture/source/parser SHA-256; include the hosted runner image version from ordinary job setup logs when comparing.
5. `node validate-output.mjs <output.jsonl>` independently validates evidence and returns 0 only for a complete failure-free run. Compare both metadata sets and summarize failures by stage and variant. A passing diagnostic does not clear the earlier required-CI failure.
6. Stop only the fixture process you started in a `finally` block, using its retained process object. Do not kill arbitrary processes by name or reuse unknown listeners. Retain unsuccessful/incomplete output with its original exit status. The workflow retains only the bounded diagnostic evidence for three days.

The low-level run-probe.ps1 script does not create, start or stop the fixture. The run-hosted.ps1 wrapper owns that fixture and stops it in a finally block. A finally block reaps only its exact owned compiler, probe and validator children after exceptions or interruption; host termination can still prevent final evidence from being written. It refuses an unexpected listener owner, and does not bypass failures by selecting another port.

## Interpretation and next decisions

- `socket` failure: native socket creation failed, before bind/connect or fixture HTTP handling.
- `nonblocking`, `event-create`, `write-event-create`, `event-select-connect`, or option failure: preparation failed; compare that API with the browser's modeled path. Variant option rejection makes that variant unavailable, not a successful negative result.
- `connect-sync` 10055 with successful setup: bare native Winsock reproduces the immediate branch without browser test code. The shared OS/provider path becomes a concrete target; that still does not identify which kernel resource failed.
- `connect-async` error: FD_CONNECT completed with an error, a distinct branch from the captured browser failure.
- HTTP-stage failure after successful connect: distinguish fixture/service/read-write health from socket-establishment failure.
- `randomized` differs reproducibly while `plain` does not under matched exposure: evidence for an option-dependent native path to minimize and report upstream, not permission to disable browser features in required CI. One passing pair cannot establish causality or absence.
- Both variants pass: native reproduction remains inconclusive; browser/process/scheduling context may matter. Preserve the original failure. A separately authorized controlled client/OS comparison is more informative than blind test reruns, extra sharding or port-range changes.

## Validation available here

Linux checks: `node --test validate-output.test.mjs`; compile/run `response-parser.test.cpp` with `g++ -std=c++17 -Wall -Wextra -Werror`. These exercise strict output shape/counters/ownership and every partial-response boundary, duplicate/framing/body/size rejection. They do not validate Windows ABI, cl warnings, PowerShell execution, actual socket behavior or any causal hypothesis. This environment has no Windows, cl, PowerShell or Windows cross-compiler; Windows compilation and both real variants remain unexecuted.

## Primary sources

- [Pinned Chromium platform socket creation](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/socket/socket_descriptor.cc)
- [Pinned client defaults and optional binding](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/socket/tcp_client_socket.cc)
- [Pinned Windows socket implementation and default event core](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/socket/tcp_socket_win.cc)
- [Pinned feature definitions](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/base/features.cc)
- [Pinned version mapping](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/base/win/windows_version.cc)
- [Official Chromium port-randomization launch discussion](https://groups.google.com/a/chromium.org/g/blink-dev/c/-Bmnni7Xv0A)
- [Microsoft connect return/completion semantics](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-connect)
- [Microsoft WSAEventSelect](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsaeventselect)
- [Microsoft socket options and SO_RANDOMIZE_PORT](https://learn.microsoft.com/en-us/windows/win32/winsock/sol-socket-socket-options)

## Hosted result boundary

The workflow does not replace or relax the existing Windows browser checks.
It requires successful native process status, complete cleanup and independent
output validation. Compilation, unsupported socket options, native failures and
incomplete evidence all fail the diagnostic lane. Both variants passing would
be an inconclusive reproduction result, not a fix for the earlier browser error.

The MSVC, PowerShell and Winsock paths remain unverified until the first hosted
run. Step deadlines leave room for evidence upload, but runner loss or job
cancellation can still prevent retention. No registry, port-range, browser
feature or persistent network setting is changed.
