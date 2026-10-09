# Staff originality history cleanup

The staff report queue links to `/robot9000-cleanup?board=r9k`. The review page
explains the retention rule and requires a separate POST to remove a batch.
Each submission removes at most 1,000 text hashes last seen strictly more than
two calendar years ago. It reports the count and whether more eligible rows
remain. Further batches require another submission. Empty batches are safe and
are recorded in the audit.

## Source and permissions

The pinned source revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.
`admin.php:693` deletes old `r9k_posts` rows inside Board Cleanup. It uses
`created_on < DATE_SUB(NOW(), INTERVAL 2 YEAR)`. Duplicate text refreshes that
column in `plugins/robot9000.php`; the rewrite uses `seen_at`. Cleanup does not
require Robot9000 to remain enabled. It does not expire mutes, delete posts or
files, or run automatically during posting.

`adminvalid('Board Cleanup')` requires target-board access and moderator rank,
then manager rank or the developer flag. `has_flag('developer')` also requires
global/no-board access. A `noboard` deny blocks that exception; target-board
denies still win for every rank. Janitors cannot use cleanup even with the flag.
The 96-case reference fixture executes the pinned rank and scope helpers in
separate PHP processes for each rank, preserving the helper's static rank cache.
Login, database operations and the legacy local-auth bypass are outside that
fixture. The modern interface uses authenticated sessions, origin and CSRF
checks, and authentication from the last ten minutes.

The active PHP does not specify the MySQL connection timezone. `README2.txt`
sets UTC but describes its schema as inferred, so it does not establish the
live service's timezone. This implementation uses UTC calendar subtraction
explicitly. It does not claim an exact historical timezone match. The cutoff is
computed after acquiring the board lock and is independent of the database
client's timezone. Two calendar years are not approximated as 730 days.

## Storage and transactions

Migration 0117 adds `content.cleanup_robot9000(text,bigint)` and an immutable
board-operation audit. The function locks the board, computes the cutoff, deletes
a fixed ordered batch, checks for more eligible rows, and appends the audit in
one transaction. It accepts neither a client cutoff nor a caller-selected limit.
Missing boards are rejected. A board that became staff-only can still have its
retained history cleaned by an authorized account; this does not grant posting
or content-reading authority. The account identifier comes from the authenticated guard, not a submitted form field.

The existing NOLOGIN Robot9000 owner receives DELETE only on the text-history
table and column-scoped INSERT on the audit. Owner-only board read/lock policies
let cleanup reach retained private-board history. Its existing narrow metadata
column grants remain; it receives no post or thread access. Public posting still
explicitly rejects private boards. Only `board_staff` may call cleanup.
Public, authentication and media runtimes cannot invoke it; runtimes cannot read
or directly delete private robot state or alter the audit. The function's fixed
search path does not resolve caller-controlled objects. The public originality
check is unchanged. A compromised staff process still has the bounded cleanup
function; account authorization remains the staff service's responsibility, as with existing guarded staff operations.

The HTTP handler holds the current account/session guard while cleanup runs and
rechecks live, recent authentication before commit. Failure, timeout, cancellation
or an audit insertion error rolls back the batch. Posting and cleanup share the
board lock: a refreshed hash is retained when posting finishes first, and text
pruned first can become original again. Other boards and all mute rows remain
unchanged. Audit records contain the account, board, cutoff, count and time;
hashes and actor fingerprints are not returned to the browser or logged.

## Scope and qualification

Anonymous staff posting with the exact prepared `bypass_r9k` option already
exists. Authentication alone does not bypass the robot. Badge selection, sage
preparation, ordinary posting and the inactive image/secret-command branches
are unchanged by cleanup.

This change implements the Robot9000 cleanup slice of the broader staff work. The
source's other Board Cleanup filesystem and side-table operations are separate.
It does not establish completion of #222 or the complete rewrite in #191.

The source fixture, staff library tests, database integration tests, role
bootstrap, populated upgrade and browser workflow must be qualified for the
published head. Local results and outstanding checks are recorded in
[the verification record](verification-staff-robot9000-cleanup.md); test definitions alone are not evidence that those checks passed.

Upgrade through the normal migrator before deploying the staff binary. An older
binary ignores the new endpoint and audit table; keeping the migration permits
binary rollback. Cleanup is irreversible without restoring a database backup.
Existing text history is untouched by applying the migration.
