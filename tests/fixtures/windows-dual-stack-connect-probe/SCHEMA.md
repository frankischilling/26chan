# Evidence schema 1

Strict JSONL, at most 2 MiB and 12,000 records; no extra keys or duplicate keys.

Header: {"type":"header","schema":1,"profile":"dual-stack-connect","mode":"plain"|"randomized","groups":40,"lanes":6,"fallback_ms":300,"group_ms":700,"work_ms":30000,"cleanup_ms":35000,"max_live":12,"max_starts":480}

Group: {"type":"group","id":0..39,"ms":nonnegative integer}. Exactly one per attempted group before any setup, using the same timestamp as its fixed deadline. Groups are sequential and start only after prior resources are closed.

Operation: {"type":"operation","id":0..479,"stage":ENUM,"result":integer,"error":nonnegative integer,"ms":nonnegative integer}
ID = group*12 + lane*2 + endpoint (IPv6=0, IPv4=1). Lanes 0..2 consume IPv6 completion when ready; lanes 3..5 defer IPv6 completion handling until close. IPv4 completion is always consumed. Endpoints are fixed media-ipv6 and media-ipv4; no addresses exported.

Stages in per-ID order:
- socket: 0/0 success, -1/error failure
- event-create: 0/0 or -1/error
- event-select: 0/0 or -1/error (FD_CONNECT, makes socket nonblocking)
- set-randomize: 0/0 or -1/error
- get-randomize: result 0 or 1 is actual boolean option value; error 0 (length must equal BOOL in producer); -1/error on API or readback mismatch
- connect: timestamp is call admission/start, also used as the IPv4 fallback baseline; 0/0 immediate success, -1/10035 would-block, -1/10061 immediate refusal (expected IPv6 only), -1/other fatal
- event-ready: emitted once only for would-block, result 0/0 if signaled or 258/0 if still not signaled at close, -1/error fatal. No polling timeout rows. Signaled row may be before close or before completion-consumed. Readiness says nothing about completion status.
- completion-api: 0/0 on successful WSAEnumNetworkEvents; -1/error on API failure (fatal regardless of numeric error). On failure no completion-consumed, close-intent remains1; enumeration is not retried.
- completion-consumed: only after signaled event-ready on consuming lane, 0/0 success or -1/error from FD_CONNECT. Missing FD_CONNECT/API failure is fatal. IPv6 10061 is expected; IPv4 must succeed.
- close-intent: 0/0 completion handled or synchronous result; 1/0 would-block completion unhandled (including signaled). Exactly once per opened socket before close.
- closesocket: 0/0 or -1/error. Exactly once per opened socket, no further socket API afterward even on failure.
- event-close: 0/0 or -1/error. Exactly once per created event, after successful socket close; if close fails event retained until process exit (no event-close).

Failure: {"type":"failure","id":0..479,"stage":ENUM,"error":positive integer,"ms":nonnegative integer}. Stage is any operation stage or startup, wsa-cleanup, work-deadline, group-deadline, cleanup-deadline. An unexpected operation error emits one failure record immediately after that operation. startup/work/cleanup deadline uses id 0; group-deadline uses group*12. Failed runs still preserve all cleanup records and final summary. Every 10055 is fatal.

Summary: {"type":"summary","starts":integer,"ipv4_successes":integer,"expected_refusals":integer,"closed_before_handling":integer,"ready_before_close":integer,"sockets_opened":integer,"sockets_closed":integer,"events_opened":integer,"events_closed":integer,"max_live":integer,"failures":integer,"operation_records":integer,"complete":boolean,"elapsed_ms":integer}
Counters derived from operations: starts connect rows; ipv4_successes odd-ID successful connect or completion-consumed; expected_refusals even-ID connect/completion-consumed error10061; closed_before_handling close-intent result1; ready_before_close close-intent result1 with preceding event-ready result0; opened successful creates; closed successful closes. max_live counts successful socket opens less successful closes. Complete requires all 480 connects, 240 IPv4 successes, no failures, all resources closed, elapsed<=35000, work starts<30000; qualification additionally requires closed_before_handling>0. Zero exposure is inconclusive and exit 1. Header first and summary last; monotonic timestamps <=40000 (watchdog bound); qualification elapsed<=35000. Any failure prevents qualification. Qualifying full traces require all setup stages and option match, both endpoints per lane, each IPv4 connect >= its IPv6 connect+300ms and within group deadline (group begins at its group row), starts within 30s, groups sequential (prior group resources closed). Summary never establishes qualification alone.

Producer exits 0 only for qualified evidence; 1 for failed/inconclusive. Validator exports validate(text) returning derived summary with qualified boolean, throws for invalid or nonqualifying evidence; CLI takes output.jsonl and writes only fixed summary JSON, exit1 on invalid/inconclusive. Test fixtures may be synthetic but must be labeled as such.
