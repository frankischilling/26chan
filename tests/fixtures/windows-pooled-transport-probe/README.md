# Pooled Windows socket lifecycle diagnostic

This opt-in diagnostic adds a six-socket keep-alive pool to the native transport
investigation. It leaves the existing plain/randomized probe, browser assertions
and required CI unchanged. A pass means the bounded native workload completed;
it does not fix, explain or clear the browser's Winsock 10055 failure.

## Workload

Each batch opens six unbound IPv4 TCP sockets to the project-owned visual
fixture at `127.0.0.1:3000`. Each socket completes six GET `/readyz` exchanges.
The loop visits sockets 0 through 5 once per exchange round. There is at most
one exchange in flight in this diagnostic. All six sockets remain owned until
the round loop ends, then the entire pool closes before event handles are
released. A failed socket is not replaced and a failed request is not replayed.
Other already planned sockets may continue; any failure makes the run nonzero.

Socket creation, nonblocking mode, TCP_NODELAY, 45-second keep-alive options,
FD_CONNECT handling and explicit `SO_RANDOMIZE_PORT` FALSE/TRUE with readback
match the existing native control. Before the first receive, the read event
changes to `FD_READ | FD_CLOSE`. Reads call nonblocking `recv`; when it would block, the
probe waits on the event and calls `WSAEnumNetworkEvents`. An empty event mask
re-arms the wait under the same absolute exchange deadline. It does not call
connect, resend a request, reset a timer or count a response. Every zero mask stays
in the native operation stream and the bounded job-log report counts them as
`empty_read_events`. Unexpected masks, closure or an event error still fail the
exchange. Completed responses must have the exact
synthetic fixture body and valid bounded Content-Length framing.

Writes use `WSASend` with an owned buffer, WSABUF, OVERLAPPED and separate
manual-reset event. The event is reset before each submission. Immediate
success and `WSA_IO_PENDING` are recorded separately. Pending writes wait only
within the original exchange deadline. `WSAGetOverlappedResult` retrieves the
terminal result and transferred byte count while the socket is still live.
A partial completed send submits only the remaining request suffix.

Chromium 151's [DidSignalRead implementation, lines 1146-1150](https://github.com/chromium/chromium/blob/151.0.7922.34/net/socket/tcp_socket_win.cc#L1146-L1150)
handles an empty mask by watching again: a synchronous read can consume data
without clearing the event signal. This diagnostic previously failed that branch
with a synthetic 10022. The e0a9c861 run reported read-event-missing failures in both variants, with
all writes synchronous and all socket/event ownership released.
The event mask itself was absent from that job log, so the empty-mask explanation
for that run is an inference. Its failed evidence remains a failure; the change
corrects a source-documented readiness case and does not establish a cause or
fix for the browser's earlier 10055.

The diagnostic remains stricter than Chromium for FD_CLOSE and event errors.
Chromium's lines 1129-1145 call recv again to obtain remaining data or a more
accurate error; this fixed keep-alive profile rejects them. No such failure is
silently accepted by the empty-mask handling.

## Close and completion ownership

A socket's close record says whether a write was still pending. Normal successful
runs finish every write before pool teardown. If a write crosses close, the
probe waits for its retained event until the fixed cleanup deadline and records
whether a signal was observed. It does not call `WSAGetOverlappedResult` with
a closed or potentially reused socket handle, inspect reserved OVERLAPPED
fields, or infer a completion error/byte count from an event signal.

Every pending-at-close case is incomplete qualification. The OVERLAPPED,
WSABUF, request buffer and write event are retained until process exit, including
when a post-close signal arrives. No new pool starts after this retention. The
summary exposes the retained ownership; it cannot pass as clean shutdown.
The same ownership rules apply when an exception unwinds the active pool.

## Bounds

- Default: 16 pools, 96 socket lifecycles, 576 exchanges. The smaller batch count
  leaves room for the extra lifecycle records under the original output cap.
- Maximum: 128 pools; at most 6 sockets and 12 event handles concurrently.
- Fixed pool start offsets: 50 ms. Late pools start without adaptive backoff.
- Connect and each exchange have an absolute 1-second deadline. Useful work
  stops at 30 seconds. Post-close observation shares a fixed 35-second deadline
  from process start. The launcher imposes the original 40-second hard limit.
- Response cap: 8192 bytes. Operation cap: 20000. Evidence cap: 4 MiB.
  Reaching a cap is incomplete evidence, not a passing shortened run.
- No threads, requests outside the owned endpoint, retries, browser feature
  overrides, registry edits, port-range changes or persistent network changes.

Evidence includes stage names, ordinal socket/exchange IDs, errors, elapsed
times, bytes, verified option booleans and ownership/completion counts. It does
not export addresses, port numbers, request/response contents or kernel traces.
The launcher records source/parser/fixture hashes and the Windows build. The
workflow records the candidate SHA and hosted runner image.

## Run and interpret

Use the same already-built owned fixture and official installed MSVC developer
shell described in the [original probe](../windows-transport-probe/README.md).
`run-probe.ps1 -Mode plain -FixtureRepo <checkout> -Batches 16` runs against that
listener. Use a separate fresh matched Windows runner for `-Mode randomized`.
The wrapper verifies that the listener belongs to the expected executable before
compilation and immediately before launch. It does not authenticate ownership
throughout the run.

The separate `windows-pooled-transport.yml` workflow runs one variant per fresh
Windows runner. It has read-only repository permissions and triggers only for
changes to this diagnostic or its workflow, plus manual dispatch. `run-hosted.ps1`
owns and reaps its fixture. Preparation, compilation, native execution, validation
and child-cleanup failures all fail the lane. Evidence is retained for three days.
No Windows run is claimed by this source change.

The launcher requests best-effort process-tree termination with `Kill(true)`
when an owned child needs forced cleanup. A compiler timeout immediately marks
`child_cleanup_complete=false`, even if the compiler exits before cleanup runs.
Status records the attempted tree kill and why cleanup remains unverified.
`WaitForExit` confirms only the root's exit; it never upgrades that result to
verified descendant cleanup. A timed-out compiler's linker descendants may
survive, particularly if the root exits first or a descendant cannot be inspected.
There is no job-object containment or independent descendant inventory here.
Forced cleanup stays unqualified and the lane fails. See Microsoft's
[Process.Kill semantics](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.kill).

`node validate-output.mjs <output.jsonl>` independently checks the record schema,
call ordering, deadlines, byte totals, option readback and ownership before it
can return success. The summary separates synchronous/asynchronous write
completions, pending-at-close writes, post-close signals and retained writes.
A run with zero asynchronous writes has not exercised asynchronous completion.
A run with zero pending-at-close writes has not exercised post-close completion.
Do not describe either as cancellation-path coverage.

The profile borrows selected calls from Chromium 151.0.7922.34's default event
core. It is not Chromium: browser requests overlap, use more endpoints and
response sizes, and run with different scheduler, process, context and cache
lifetimes. This probe deliberately serializes exchanges and uses one fixed
health response. The optional IOCP core is not modeled. Its feature is disabled
by default in the pinned source, but the retained browser stderr does not prove
which core was active. Both cores share the connect implementation. Explicit
FALSE option application and readback remain differences from Chromium's
feature-off path. Chromium also passes a byte-count pointer to WSASend and
checks whether the event is signaled before taking its synchronous fast path.
This diagnostic follows Microsoft's current null-pointer recommendation and
asks WSAGetOverlappedResult for both immediate and pending completions; it
records those paths separately. It accepts synchronous connect success, which
the pinned Chromium implementation marks unexpected. One passing pair remains
an inconclusive reproduction.

## Bounded job-log diagnostics

The launcher captures its original qualification exit code, then runs
`node validate-output.mjs --diagnostics <evidence-directory>` as a separate owned
reporter. Reporting cannot turn a failed run into a pass or change the captured
qualification exit. The outer launcher also fails if the reporter cannot start,
returns nonzero, times out, or has unverified cleanup. This can add a failure; it
can never clear the original one. Both exit decisions appear in the fixed reporter
record. The native 30-second work budget, 35-second cleanup window,
40-second launcher deadline and all original checks remain unchanged. Reporting
has its own 10-second deadline and a maximum 5-second root reap after a best-effort
process-tree kill. Forced reporter cleanup is explicitly unverified.

The reporter emits a fixed JSON record containing validated launcher booleans and
exit codes, a bounded native summary, at most 16 native failure stage/error/socket
ordinal records, and at most 16 MSVC error codes with source line numbers. Linker
errors have a null source line. It strips paths, compiler messages and other
arbitrary text. Launcher status is limited to 8 KiB, native evidence to 4 MiB,
and each compiler stream to 64 KiB; reads are bounded and symlink files are
rejected. Missing or rejected launcher status leaves every native outcome unknown.
Malformed native evidence is never labeled a successful validation. A shape-valid
but semantically rejected trace can expose safe counters and failure records,
with a fixed validator-problem code, for diagnosis only.

Raw reporter stderr remains in the evidence directory. A separate fixed record
reports reporter exit, timeout and cleanup state; the hosted wrapper reports
fixture cleanup. These records make a failing phase visible when artifact
retrieval is unavailable. They do not reproduce the raw artifact, weaken normal
validation, establish a cause for 10055, or replace retained failed evidence.

## Portable checks

Run `node --test tests/fixtures/windows-pooled-transport-probe/validate-output.test.mjs`
and compile/run `response-parser.test.cpp` with
`g++ -std=c++17 -Wall -Wextra -Werror`. Run the original validator tests as well.
These cover evidence and framing; they do not qualify the Windows ABI, MSVC
warnings, PowerShell, actual event delivery or a causal hypothesis. Native
compilation and both runtime variants require hosted Windows qualification.

## API references

- [Pinned Chromium Windows socket implementation](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/socket/tcp_socket_win.cc)
- [Pinned Chromium feature defaults](https://chromium.googlesource.com/chromium/src/+/151.0.7922.34/net/base/features.cc)
- [WSASend: buffer lifetime, pending and immediate completion](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsasend)
- [WSAGetOverlappedResult: live socket and terminal results](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsagetoverlappedresult)
- [WSAEventSelect: event registration and reenabling](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsaeventselect)
- [closesocket: cancellation does not imply completed I/O](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-closesocket)

The same pinned implementation is available in the [official GitHub mirror](https://github.com/chromium/chromium/blob/151.0.7922.34/net/socket/tcp_socket_win.cc).
