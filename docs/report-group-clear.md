# Ordinary report-group clearing

Migration 0108 adds a board-scoped clear transition for one post's current report
group. It is separate from resolving one report and from reporter-wide clearing.
The reference is the supplied `ReportQueue.php:2563–2734` and
`modes/report.php:613–642`, revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.

## Eligibility and current limits

A janitor or higher must have access to the target board and recent
authentication. Explicit board denies still apply. Every current member must
have captured, non-NULL effective-weight evidence, and the sum must be nonzero.
Unknown weights are rejected without changing the group. The action is bounded
to 10,000 members and never clears a truncated subset.

Current weight proofs support the existing opt-in categorical path when the
configured category base is 0.5. The default free-text path and historical
unknown weights cannot qualify. No category catalog is supplied or activated by
this migration, and no weight is inferred or backfilled. The source's weighted
cross-board unlock remains unavailable.

## State and concurrency

The private group lifetime records the originating clear time and account.
Exactly its current reports receive immutable clear evidence; their original
resolve/dismiss state, memberships, counters, quotas and weight evidence remain
unchanged. One count-checked moderation audit is committed with the transition.
A repeated clear returns an already-cleared conflict without another audit.

All three existing admission overloads pass through the membership insertion
trigger. New reports inherit a surviving lifetime's clear evidence, even if their
own weight is unknown. They cannot reopen it. Partial reporter retirement keeps
that lifetime state. Removing its last member deletes the lifetime; a later
fresh report starts an uncleared one. Retained old report history never seeds a
new lifetime.

The helper locks the target board before group or report rows. Existing
admission, archive, deletion and reporter-retirement paths serialize through that
board. It acquires no additional global admission gate or identity-session lock.
The application holds its authority guard and rechecks current recent
authentication immediately before commit. Any marker, count or audit failure
rolls back the whole transition.

New fields have no defaults or historical backfill. Only the private admission
owner can change the clear markers; runtime staff calls the bounded function.
The function checks its exact staff invoker and uses a fixed search path.
Readiness verifies columns, constraints, ownership, ACLs, enabled trigger binding
and the bounded-history index.

## Interface and history

POST `/report-group-clear` accepts only `csrf`, `board` and `post_id`, with the
same strict origin, content-type and body bounds as reporter clearing. The
active queue excludes group-cleared reports, and resolve/dismiss cannot target
them. Ordinary forms perform the group action immediately.

GET `/reports/cleared?board=...` shows at most the newest 100 records for one
allowed board. It shows report evidence, original disposition, originating clear
account/time and whether the report inherited that clear. It does not show a
current post as a historical snapshot. Reporter-wide retirement also hides rows
from this history; retained database evidence is not erased.

## Remaining source behavior

This is a partial capability. Complete effective weights, weighted cross-board
unlock, already-cleared orphan hard deletion, staff-self-clear identity logging,
report-time post snapshots and identity-based abuse warnings remain incomplete.
The source queue's grouped display, three-second delay and undo behavior are also
not implemented by these ordinary forms. This does not complete report or staff
parity.

## Qualification

The owned migration exercise covers populated 0107-to-0108 and fresh installs,
nullable retained history, real admission inheritance, current readiness drift,
runtime denials, bounded failures and administrator dump/restore. The old 0106
reporter-clear exercise uses its frozen readiness query so it still verifies its
historical boundary. Current 0108 readiness is checked separately.

Local fresh migration and both runtime readiness checks passed. The seven store
and seven staff integration cases pass with one and eight test threads; fixture
mutexes isolate independent singleton state, with deliberate races inside cases.
The store cases include archive/deletion retirement against a blocked clear call.
Populated/fresh 0108 upgrade and restore, the frozen 0106 boundary, and the 0102
forward-to-current weight exercise also pass.

Combined local qualification covers 222 store, 126 selected public and 152 staff
checks. The broader runner was interrupted during its last staff posting target;
that complete supported target was rerun successfully. Three Unix-socket cases
are excluded only locally after executor permission failures and remain enabled
in CI. Formatting and strict all-target, all-feature workspace Clippy pass.
At `2eb117f`, hosted media/operations, Windows, all four theme shards, monitoring
and dependency checks passed. Linux passed the Rust and database checks but
failed a later watcher fixture cleanup: a deleted thread's JSON GET returned 503
instead of 404. Its exact storage error was not captured, so full hosted
qualification remains incomplete. Local migration adaptation uses loopback
TCP because this executor does not support the scripts' Unix-socket setup;
hosted CI runs the original scripts.
