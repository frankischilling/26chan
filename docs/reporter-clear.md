# Reporter-wide report clearing

Migration 0106 and `POST /reporter-clear` add a bounded staff action for clearing
reports linked to one captured reporter. The action removes active report
membership and hides those reports from the queue. It retains the report records
and their evidence. This is partial source parity for IP and automatic-session
equality, with stricter authorization, transaction handling and audit.

## Source contract

The supplied `4chan-old/reports/ReportQueue.php:1891-1971` implements
`clear_reporter()`. A report ID selects its IP, password and Pass ID. If no ID is
supplied, a direct IPv4 input can select an IP instead. The delete predicate is
one flat OR: the seed IP, its nonempty password, or its nonempty Pass ID. Matches
span boards, posts, categories, ages and cleared states. A matched report's other
identities do not expand that predicate.

`4chan-old/reports/access.php` grants this capability to moderators, managers and
administrators, not janitors. `4chan-old/lib/admin.php:129-145` applies board
restrictions to janitors; source moderators have global report authority.

The source physically deletes matching `reports` rows. Its
`clear_orphaned_reports()` helper, at lines 970 onward, then deletes aggregate
rows only when no report remains for that board/post. Partial clearing does not
subtract from the aggregate's recorded counts. The cleared-reporter logging call
in `clear_reporter()` is commented out. This action does not issue an abuse ban
or perform the ordinary post-group clear workflow.

`4chan-old/reports/js/d8d9b0cdc33f3418/reportqueue-mod.js:233-257,420-464` exposes
"Clear All" in reporter details, sends the report ID or IP with a CSRF token,
displays the affected count and closes the panel. Source dispatch validates CSRF
and the exact host before calling the action (`ReportQueue.php:2889` onward).
The separate `clear_report()` at line 2563 has post-group access, weight and
logging behavior; it is not the contract for reporter-wide clearing.

## Staff request and response

The rewrite uses a separate POST endpoint rather than generic moderation action
dispatch. The URL-encoded body accepts exactly `csrf`, `board` and `report_id`,
with no duplicates or unknown fields. Its maximum size is 4,096 bytes. The board
must be a valid board slug, the ID must be a positive decimal signed-64-bit
value, and CSRF must be nonempty and at most 256 bytes. Invalid percent escapes
and invalid UTF-8 are rejected. The form cannot supply an IP, identity digest,
session identifier, password or Pass value.

The endpoint requires the existing origin and CSRF checks, exactly one
`Sec-Fetch-Site` header, and exactly one URL-encoded `Content-Type` header.
Authority must be moderator level or higher, explicitly allow `all`, and have no
denied boards. A board-scoped moderator cannot submit a narrower version of
Clear All. Janitor developer flags do not grant this authority. The action
holds the live staff authority guard and requires recent authentication, then
rechecks current and recent authority immediately before committing content.

Successful requests return HTTP 200 HTML headed `Cleared N reports`, using the
exact affected count, with a link to `/reports`. The page is rendered before the
final authority check and commit, and returned only after those succeed. A
missing seed membership, wrong seed board, historical unknown ownership or
already-retired seed returns 404. Invalid forms return 400; route body overflow
returns 413. Insufficient scope and stale recent authentication return 403.
Database limit and lock-conflict failures return the existing generic 503
response, without exposing private identities or database exception details.

## Matching and retained evidence

`content.clear_reporter(text,bigint)` looks up the seed in private active report
membership. Its flat predicate matches the captured `actor_hash`, or the seed's
nonnull `automatic_identity`. NULL automatic identities do not match one
another. A match on both branches counts once. The seed's captured equality can
survive reporter session expiration and cleanup. The action does not require a
live reporter session, report time window, open report state or live target post.
It never reconstructs ownership from retained history.

The function deletes matching memberships and marks exactly the corresponding
retained report rows with `reporter_cleared_at`. It leaves their original state,
category, weight evidence and other original fields unchanged. Anonymous session
history and earlier audit records also remain. The staff queue and ordinary
resolve/dismiss selection exclude marked reports.

Keeping these records is a deliberate backend safety difference from the
source's physical deletion. Ordinary resolve/dismiss still changes report state
without retiring admission membership. Reporter clearing releases the removed
memberships' duplicate, quota and capacity contributions.

The existing membership deletion trigger preserves a surviving group's
`illegal_count` and sticky `incomplete` flag. Only removal of the last member
ends that group lifetime. New reporting then starts a new lifetime; retained
report history does not recreate old counts. See
[report-group lifetimes](report-group-lifetimes.md).

## Transaction, limits and audit

The helper is a `SECURITY DEFINER` function owned by
`board_report_admission_owner`, with fixed `pg_catalog,pg_temp` search path and
an explicit `board_staff` invoker check. Execution is granted to staff only;
public and authentication runtime roles cannot call it. Staff does not gain raw
private membership access or direct permission to update the report marker.

A fresh Read Committed content transaction takes every current board lock in
canonical slug order before taking the report-admission gate. It then reads the
seed and matching memberships in fresh statements after lock waits. The maximum
is 512 current boards and 10,000 matching reports. A limit overflow aborts the
operation; it never clears a truncated subset.

If a matching report belongs to a newly introduced board outside the recorded
locked set, the transaction aborts rather than acquiring another board lock
after the gate. Retrying requires a fresh transaction and the full lock protocol.
The function checks that the actual deleted membership IDs and marked report
count agree. Trigger, audit, authority or content commit failures do not leave a
partially committed clear.

The content transaction inserts one `moderation_audit` row with action
`reporter-clear`, the staff `account_id`, seed `board`, seed report ID in
`target_id`, and `reporter_clear_count` from 1 through 10,000. Snapshots and masks
remain NULL. The audit contains no reporter IP, hash or session identity. This
atomic audit and transactional cleanup are hardening additions; they are not
claimed to exist in the source action.

## Remaining gaps and qualification

This action does not implement legacy password equality, Pass equality, direct-IP
input, unknown or retired ownership seeds, or the complete source reporter popup
and history interface. It does not restore identity equivalence across key
rotation. Those limits prevent a claim of complete reporter-clear parity.

Apply migration 0106 before starting matching public and staff binaries. Keep the
forward schema and audit evidence during rollback. Older staff binaries ignore
the marker and are not compatible active-queue readers after clearing has been
used; a binary-only downgrade does not preserve this behavior.

Local Linux qualification passed 475 selected Rust cases: 213 store, 126 public
and 136 staff. Three unavailable Unix-socket cases were excluded locally and
remain enabled in CI. Formatting and strict all-target workspace Clippy passed.
The shared readiness query passed under both actual runtime logins.

All nine reporter-clear integration cases passed with one and eight test threads.
The harness serializes its global fixtures; concurrency is exercised explicitly
inside cases, including admission, archive/deletion, a newly committed matching
board, and final authorization expiry while waiting. These checks do not claim
that nine global clears ran simultaneously.

Fresh and populated 0105-to-0106 upgrades and administrator dump/restore passed,
including runtime privilege denials, readiness drift, retained evidence and exact
audit shapes. Rollback-only fixtures with 513 boards and 10,001 matching reports
confirmed that both caps fail before mutation. Normal triggers remained enabled.

Exact-head hosted CI and browser qualification remain pending. No original
application or live operator data was executed; the source contracts above come
from inspection and the implementation checks use owned synthetic fixtures.
