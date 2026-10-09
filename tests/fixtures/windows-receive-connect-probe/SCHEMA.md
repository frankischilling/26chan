# Receive/connect evidence schema

The native output is newline-delimited JSON with one header, operation/option/failure records, and one terminal summary. Schema version 1 uses exact field sets. Unknown fields, duplicate JSON keys, missing terminal newlines, oversized records, inconsistent counters, and false completeness claims are rejected. The executable validator is `validate-output.mjs`.

## Native records

The header identifies the fixed `receive-connect-history` profile, `serialized` or `overlap` schedule, Boolean randomization setting, pool width/count, exchange count, retirement interval, work/cleanup limits, response/operation limits, and descriptive boot timestamp.

An operation contains `type`, `id`, `pool`, `lane`, `exchange`, `stage`, `result`, `error`, `ms`, and `boot_ms`. Socket IDs are contiguous from 0 through 120; pool and lane are derived from the ID. Exchange -1 is setup; exchanges 0 through 5 identify requests. `ms` is elapsed steady-clock time rounded down to milliseconds. `boot_ms` is descriptive and is not used to synchronize with the fixture.

An option record contains `type`, `id`, `value`, `length`, and `ms`. It follows the socket's randomization readback. Complete qualification requires the requested Boolean and a four-byte option result.

A failure contains `type`, `id`, `stage`, `error`, and `ms`. Error zero is reserved for missed-exposure markers. Harness deadline and ownership codes must not be treated as observed Winsock failures without a corroborating native operation. After the first failure, only cleanup work is accepted. Native failure markers must match a recorded API failure; missed-prefix and premature-final markers require a completed streaming response and consistent prefix state. Failed runs receive ownership, counter, and failure-prerequisite checks, not the complete successful-lifecycle grammar.

The summary reports connection attempts, completed responses/pools, failure count and first error, socket/event ownership counters, peak live sockets, operation count, completeness, elapsed time, synchronous/asynchronous write counts, pending and retained writes, post-close signals, and transferred bytes. Counts are recomputed from the trace. A complete success requires 121 starts, 721 responses, 20 full pools, 121 opened/closed sockets, 242 opened/closed events, peak six sockets, no retained writes, and no failures.

## Fixture records

Each fixture record contains `schema_version`, `scope`, `sequence`, `pool`, `request_id`, `lane`, `event`, `monotonic_ns`, `unix_ms`, `emitted_bytes`, `output_failed`, and `emission_boundary`.

The scope is `fixture-transport-overlap`; the boundary is `http_body_poll`. Each pool has `accepted`, `prefix_emitted`, `suffix_emitted`, and `body_complete` in order, or terminates early with `body_dropped`. Emitted-byte totals are 0, 1,024, and 4,096. The suffix must follow the prefix by at least 250,000,000 ns on the fixture's own monotonic clock. Wall-clock timestamps are descriptive only. Missing or dropped bodies cannot qualify an arm.

## Validation outcomes

- `qualified-bounded-success`: the entire workload, ownership, schedule, fixture, and exposure checks passed.
- `observed-native-10055`: a recorded native API operation reported 10055. This alone does not establish an operating-system cause.
- `observed-native-failure`: another native operation failure or unexpected transport closure was observed.
- `harness-or-deadline-failure`: the probe failed an internal bound or control without a corresponding native API failure.
- `inconclusive`: complete exposure evidence is absent or the intended timing window was missed.

Malformed native evidence is rejected rather than assigned an outcome. Fixture validation is separate: `fixtureEvidence.state` is `complete`, `incomplete`, `missing`, `invalid`, or `unavailable`, with a bounded `problem` code when it is not complete. Invalid, missing, unreadable, or incomplete fixture evidence prevents qualification but cannot erase an independently validated native failure. The validator exits 0 only for qualified bounded success, 1 for valid native evidence that is unqualified (including fixture errors), and 2 for invalid native evidence or invalid invocation. Its report preserves `firstFailure` separately from `firstNativeOperationFailure`, plus workload, exposure, and fixture status.

The launcher records its own timeout, listener, compilation, and cleanup results in `status.json`. It preserves a validated native failure even if listener rechecking or later cleanup also fails. A harness failure always prevents launcher qualification; inspect both the native validation and launcher status.

## Timing claim

The overlap anchor is the first `write-submit-bytes` marker emitted before `WSASend`, including when a request needs partial writes. Every sibling connect return must fall strictly below 200 elapsed milliseconds from that anchor, after the positive incomplete 1,024-byte receive and before final body consumption. The independently recorded server hold is at least 250 ms. Request causality and this margin establish that the suffix had not yet been handed to HTTP when those calls returned, without comparing independent clock origins. They do not prove simultaneous kernel operations.
