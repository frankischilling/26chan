# Report-group lifetimes and archive retirement

Migration 0100 tracks forward report-group lifetimes in a private table. A group
is the surviving admission membership for one board/post pair. Retained report
records and audit history are separate and remain unchanged.

## Source behavior

The supplied `modes/report.php:648–653` increments persistent report counts.
`ReportQueue.php::clear_reporter()` removes matching report rows without
subtracting their contributions from a surviving aggregate; its orphan cleanup
removes the aggregate only when no report remains. `imgboard.php::archive_thread()`
clears all report kinds for each post with fewer than three recorded illegal
reports. At three or more, it preserves the whole group.

A live-row recount would lose that behavior after a partial reporter purge.
The private counter therefore keeps illegal contributions for the lifetime of
the group. Three illegal reports followed by one rule report still count as
three after those illegal memberships are removed. Removing the final membership
ends the group; a later report starts a new lifetime.

## Forward tracking

Successful membership insertion increments the counter only for category kind 2.
Rule reports add no illegal contribution. Free-text or otherwise unknown kinds
set a sticky `incomplete` flag. Partial removal never clears that flag or reduces
the count. Identity registration does not increment the counter again.

Installation creates an empty table. It does not classify or count historical
memberships. A missing counter is unknown. If a later insertion finds older
memberships outside its own insertion statement, it creates an incomplete group,
even when those older rows have category metadata. Removed historical
contributions cannot be reconstructed safely. Only actual emptiness resets that
uncertainty; retained report history does not seed a new group.

Statement triggers process actual inserted/deleted rows, including bulk and
zero-row operations. An ignored conflicting insertion contributes nothing.
This is a deliberate hardening difference from the PHP sequence, which can
increment its aggregate after `INSERT IGNORE` without checking affected rows.
Bigint overflow fails the transaction rather than wrapping below the threshold.

## Archiving and locking

Only a transition from unarchived to archived runs archive retirement. For each
post independently, it removes every membership when the counter is present,
complete and below three. Unknown, incomplete and higher-count groups survive.
Resolved and dismissed memberships participate in the same rule. Repeated
archive writes and later reports on an already archived thread do not trigger
another retirement. Whole deletion and existing file-removal retirement paths
also remove empty groups through the membership deletion trigger.

Maintenance requires Read Committed. Normal admission and archive paths already
hold their board locks. Operators performing bulk membership writes must first
lock every affected board in canonical order and revalidate the target set.
The statement triggers reenter those locks with NOWAIT before changing counters;
they never take the report-admission gate or an anonymous-session lock.

AFTER triggers cannot intercept an earlier foreign-key or tuple wait. Enabled
triggers and Read Committed alone do not make unlocked bulk maintenance safe.
A trigger that reaches a conflicting board fails promptly, but the required
board-first protocol prevents reverse multi-thread waits before that point.

The table and trigger functions belong to the existing private report-admission
owner. Runtime roles cannot read or mutate counters or invoke the trigger helpers.
Readiness verifies their columns, constraints, permissions and exact enabled
trigger bindings. Dump/restore qualification checks the stored lifetimes.

## Limits and operations

Counter rows track nonempty membership groups and share their existing storage
bound. There is no historical sweep, category inference, or periodic reevaluation
of already archived threads. Direct owner edits, disabled triggers, TRUNCATE and
membership-target rewrites are outside the supported runtime mutation paths.

The counter does not add reporter-purge authorization, hard-clear workflows,
weighted queue priority, or exact staff-self-delete distinctions. Those remain
separate parity work. Existing staff file-removal behavior is unchanged.

Apply through migration 0100 before starting matching services. Exact execution
results belong in the pull-request checkpoint; this contract alone does not
claim every platform or browser is qualified.
