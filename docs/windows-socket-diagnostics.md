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
