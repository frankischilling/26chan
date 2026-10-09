# Windows socket diagnostics

## Recorded connection failure

At `38e653a`, [theme shard 2](https://github.com/frankischilling/26chan/actions/runs/37550690150/job/112565173343)
passed 263 cases and failed one catalog initialization prerequisite. The main
interaction script was not loaded, so the Settings readiness attribute was
absent. Style comparisons were not reached.

The retained NetLog connects request 21724 for `/static/thread-watcher.v1.js`
through stream 21727 and connection job 21728 to socket 21729. Its attempt to
connect to `127.0.0.1:3000` returned Windows error 10055 at
2026-10-07 00:16:22.377 UTC, followed by Chromium error -176. The same script
returned HTTP 200 with 62,786 bytes 371 milliseconds earlier. Other connections
succeeded in the failure's millisecond and immediately afterward.

This explains that failed initialization without demonstrating a JavaScript or
CSS defect. It does not identify the underlying Windows resource or queue.
Bracketing samples showed 820 to 1,087 dynamic IPv4 ports in use out of 16,384,
about 12.4 GiB free physical memory and about 219 MiB nonpaged pool. Those samples
cannot rule out a transient or local limit. TCPIP exhaustion-event counts were
unavailable, not zero. [Microsoft's error description](https://learn.microsoft.com/en-us/windows/win32/winsock/windows-sockets-error-codes-2)
does not specify which resource failed.

The [failure artifact](https://github.com/frankischilling/26chan/actions/runs/37550690150/artifacts/11452842205)
retains the bounded NetLog, trace, screenshot and resource evidence. Assertions,
image prerequisites, screenshot tolerances and retries were not relaxed.

## Read-only manifest descriptors

The previous inventory retained all 72 eligible templates and 417 field
occurrences, but its public metadata lacked exact numeric channel IDs and raw
64-bit keyword masks. No runtime AFD events were captured. Empty event counts
in that summary do not establish absence of kernel failures.

The next collector uses [TdhEnumerateManifestProviderEvents](https://learn.microsoft.com/en-us/windows/win32/api/tdh/nf-tdh-tdhenumeratemanifestproviderevents)
for the verified AFD provider GUID only. A size probe and one bounded fetch read
manifest descriptors. Native allocation is capped at 262,144 bytes and 4,096
descriptors. Returned sizes, header and array bounds are checked before reading.
Only descriptors matching the eligible manifest event identities are retained,
with a separate 512-entry cap.

Descriptors join by provider GUID, event ID and version. Missing or ambiguous
matches, contradictory level/opcode/task values and invalid bounds stay
unavailable. Channel is an unsigned byte; keyword masks use fixed-width
hexadecimal strings so zero and high bits survive JSON. Display names and
keyword labels are not used to reconstruct raw values.

The existing 262,144-byte private-input and 65,536-byte output limits still apply.
Raw-descriptor completeness is separate from public metadata completeness.
Neither is runtime capture readiness.

This reads metadata only. It does not start or enable a trace, register a
provider, load a manifest, collect event payloads, change network settings or
export raw pointers or addresses. Capture remains unavailable and runtime
completeness remains false. The 56 Node metadata/NetLog cases passed locally. PowerShell/C# decoder fixtures
could not execute in this environment; both those fixtures and actual Windows
collection require exact-head hosted qualification.

## Remaining boundary

Exact descriptors do not establish a payload decoder or explain error 10055.
The retained manifest includes address-bearing and pointer-bearing templates;
level and keyword filters alone do not isolate the small error-only subset.
Any later runtime design needs verified collection-side filtering, complete
schema/version/length validation and safe correlation before collection. A
filtered exported summary is not a substitute for that boundary. NTStatus and
Winsock error numbers must not be treated as interchangeable.

## Hosted descriptor verification

At `2fd227b`, the Windows synthetic decoder checks passed. The
[retained inventory](https://github.com/frankischilling/26chan/actions/runs/37554201269/artifacts/11454352751)
contains all 72 eligible events, 417 field occurrences and 55 deduplicated
descriptors. All 55 descriptors now have validated numeric channel 16 and exact
64-bit keyword masks; the raw descriptor status is `ok`, with no API error or
raw descriptor issues. Public metadata and schema rejection counts are unchanged
from the preceding inventory. No input or output truncation is reported.

All four Windows theme shards passed on that revision. Runtime capture remains
unavailable, and this successful run does not resolve the intermittent 10055
cause. The overall run failed later in an unrelated Linux mobile Settings test.

At `4e684b4`, Windows theme shard 3 passed 383 cases and failed one Tomorrow
catalog viewport case. The stylesheet request for `theme.css?worksafe=false`
failed before HTTP. NetLog request 45232, stream 45235, connection job 45236 and
socket 45237 show TCP connect error 10055 and Chromium -176. The screenshot uses
fallback Yotsuba colors across the whole page; this is not an isolated canvas
paint discrepancy. The page had `disableAll: true` and no Settings interaction,
so first-run native Settings initialization was inactive.

The viewport fixture now checks its existing failed-stylesheet collection before
pixel comparison, making transport failure explicit. Expected pixels, retries
and timeouts are unchanged. This improves diagnosis and does not fix the
unidentified Windows resource condition.

## Bounded workload isolation experiment

Build `37712005765` failed Windows theme shard 3 twice: the original run lost
the document connection, and its single unchanged rerun lost `/static/theme.css`
while the document and other assets loaded. Both retained NetLogs show Windows
10055 and Chromium -176 before an HTTP response on the affected connection.
The cases occupied positions 289 and 269 of the 384-test shard. Other connects
succeeded immediately around the second failure. This does not establish an
exhausted resource, a cumulative threshold or an operating-system fix.

The next isolation experiment uses eight fresh runners and makes only the
independent file-state, page-chrome and viewport reference matrices shardable
at the test level. One worker, zero retries, all assertions, original timeouts
and the four-NetLog-file limit per shard remain unchanged. Lifecycle suites
keep their existing grouping. Merely doubling the old file-level shard count
left the failing 384-test workload intact and produced three empty shards.

Run `node scripts/check-windows-theme-shards.mjs` on Linux or Windows to discover
the complete suite and each shard without starting a browser or fixture server.
The guard compares the exact relative-file/project/full-title identity union
against current unsharded discovery, rejecting focused (`test.only`), missing,
extra, duplicated or skipped cases, fail/slow annotations and empty shards.
It checks project-level retry configuration; current theme sources were also
reviewed to contain no per-suite retry overrides, which the JSON reporter omits.
Every runner performs this preflight before building its fixture. Each shard
must contain at most 160 tests; growth
above that ceiling requires reviewing the partition, never silently filtering
tests. The current 1,185 cases partition as 152, 145, 148, 148, 148, 148, 148, 148.

This doubles runner setup and fixture-build instances from four to eight while
reducing each browser/server workload. Maximum retained NetLog capacity is still
48 MiB per shard, now 384 MiB across eight shards. Discovery is scheduling
evidence only; actual Windows browser execution must qualify the experiment.

## Offline AFD error-profile review

The retained inventory artifact `11602268218` contains one 39,436-byte
`summary.json`. It reports 72 templates, 417 field occurrences and complete raw
manifest descriptors, with no metadata truncation. Its runtime `events` array
is empty and `complete` is false. This is manifest evidence only.

`scripts/windows-afd-error-profile.mjs` reviews four exact candidate identities
from that inventory. All are version 0, channel 16, level 4, task 0 and opcode 0,
with keyword mask `0x8000000000000000`:

- Event 1: socket creation, including accepted or reused socket handles.
- Event 6: connect completion. It can report success.
- Event 13: socket close. It can report success.
- Event 40: failed bind.

These meanings follow Microsoft's
[Winsock event reference](https://learn.microsoft.com/en-us/windows/win32/winsock/winsock-tracing-event-details).
The exact field types and order come from the retained manifest. Events 6, 13
and 40 have `Process` and `Endpoint` pointers followed by `Error`, a
`win:UInt32` displayed as `win:NTStatus`. Event 1 has the two pointers, then
`AddressFamily`, `SocketType` and `Protocol` as `win:UInt32`, then pointer-sized
`UserModePid`. The existing broad inventory gate still marks `AddressFamily`
unreviewed. The separate candidate validator checks its exact shape without
relaxing that gate or approving other templates.

The validator accepts at most 65,536 bytes of inventory JSON. It requires
complete, consistent counts and valid dictionary references, then checks each
candidate's descriptor, version, field order, scalar types, presentation types
and attributes. Missing, duplicated, changed or partial candidate schemas fail
closed. A valid result sets `metadata_profile_valid: true`; it always leaves
`capture_ready: false`, `complete: false` and `live_layout_qualified: false`.

### Synthetic decoder checks

Run the offline checks with:

```sh
node --test tests/browser/windows-afd-error-profile.test.mjs tests/browser/windows-visual-afd.test.mjs
```

The decoder accepts constructed test records only. There is no payload-file
command or runtime collector integration. CI runs these fixtures in its existing
Node diagnostic test step; live collection and artifact handling are unchanged.
The `synthetic` origin marker is a caller assertion, not a way to authenticate a
record's origin. Do not pass runtime payloads to this API.

Fixtures use a packed little-endian contract with explicit 4-byte or 8-byte
pointers. Creation has 24 or 36 bytes; completion, close and failed bind have
12 or 20 bytes. These sizes are synthetic assumptions. They do not qualify
native event-header flags, architecture selection, offsets or live payload
layout on a Windows runner.

Each invocation accepts at most 128 records, 32 active synthetic socket
identities and 36 bytes per payload. Output is capped at 32,768 bytes. A bad
record or exceeded limit rejects the entire run and discards partial output;
limit failures explicitly mark truncation. Tests cover short and oversized
payloads, unknown versions, descriptor drift, high-bit NTSTATUS, 64-bit identity
values, pointer-width separation, reuse and missing lifecycle events.

Synthetic process/endpoint pairs stay in invocation-local memory. Creation
assigns a run-local socket ordinal; another creation for the same pair starts
a new generation. Close retires it, and a later error without a new creation
is unmatched. Output contains only the ordinal, event ID, correlation label
and, where present, an unsigned hexadecimal NTSTATUS and its failure bit.
Pointers, addresses, payloads and PID values are never exported. No persistent
identity map, hash or native process role is produced. A successful synthetic
run sets `synthetic_qualified: true` while all capture and live-layout gates
remain false. NTSTATUS stays separate from Winsock error 10055.

### Gates before any separately authorized activation

1. Qualify the native decoder on the exact Windows build and architecture.
   Verify event-header pointer-width flags, descriptor identity, actual payload
   lengths, field offsets and unsigned status handling. Reject unknown or
   contradictory layouts before reading fields. Synthetic fixtures and manifest
   descriptors alone cannot satisfy this gate.
2. Verify collection-side event-ID inclusion filtering for exactly 1, 6, 13 and
   40. Microsoft's
   [EnableTraceEx2 reference](https://learn.microsoft.com/en-us/windows/win32/api/evntrace/nf-evntrace-enabletraceex2)
   documents `EVENT_FILTER_TYPE_EVENT_ID` and replacement of the filter set on
   subsequent enable calls. Level and keyword alone also admit address-bearing
   templates. Require the filter on every enable/update path, and prove that
   failure, unsupported filtering or missing filter data stops collection.
   Export-time filtering cannot provide that boundary. Event-ID filtering also
   does not eliminate the provider's cost of generating excluded events.
3. Verify native process lifetime and role correlation independently. A kernel
   process pointer is not a user-mode PID. A socket creation does not prove a
   Chromium or server role, and pointer/PID reuse needs lifetime evidence.
   Missing creation or close events, process changes and ambiguous identities
   must remain unmatched. Timestamp proximity to NetLog is only a candidate
   association, even with a validated clock bridge.
4. Review bounded in-memory retention and sanitized output, including loss,
   overwrite, truncation, session shutdown and crash cleanup. Unknown loss
   counters cannot be reported as zero or complete. Require explicit limits
   before accepting any live events; do not retain raw payloads, ETL files,
   stackwalks, pointers, addresses or PIDs in artifacts.

No trace is enabled by this work. Native role correlation remains unverified,
and the cause of Windows error 10055 remains unresolved.
