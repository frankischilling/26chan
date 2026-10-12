# Archive storage and complete JSON lists

The original `trim_archive()` removes archives by age. It does not cap the number
of retained archives. `generate_board_archived_json()` selects every archived OP
in ascending post-number order. The HTML archive page has a separate 72-hour
window and a 3,000-entry limit.

The rewrite no longer uses the legacy `archive_limit` setting to delete stored
threads. That column remains for schema and configuration compatibility, but has
no effect on new OP admission. Disabling archiving hides the archive routes and
stops automatic archive cleanup; it does not erase previously stored archives.
Re-enabling it applies the current age policy to retained entries before making
them available again.

Rollover sets the archived thread's root/bump clock to its archive time, as the
source does. It leaves the source `last_modified` clock unchanged. HTTP validators
still use the separate HTTP modification clock. A thread neglected for more than
three days can therefore appear at the top of the HTML archive when it rolls over.

## Reader limits

The JSON list includes retained entries older than the HTML window. It does not
silently stop at 1,000 IDs. The database reader rejects the whole request if it
would load more than 100,000 entries or more than 8 MiB of subject bytes. These
are backend resource limits, not source archive-retention rules. The HTTP layer
also keeps its configured response and shared-buffer budgets. An oversized
response fails with 503 instead of returning a partial list.

Each archive thread looks up its unique OP directly. This prevents stale board
statistics from reversing the join into repeated full-board scans. Both JSON
and HTML retain the database role's five-second statement limit and existing
visibility checks.

Board policy, entry count, byte count and rows are read in one repeatable-read
transaction. A conditional request still checks visibility before returning 304.
A matching ETag cannot expose a disabled or private board.

## Compatibility and remaining work

Migration `0138_archive_age_policy.sql` applies the current board age to live
retained archives. Changing a positive age updates their saved deadlines from
`archived_at`; it does not reset their archive clocks. Shortening the age can hide
an entry immediately. Lengthening it can expose an entry that is still stored,
but cannot recover a thread already deleted or erased. Disabled boards keep their
saved deadlines until an operator enables archiving again.

The board setting keeps its existing encoding: zero disables archiving and a
positive value specifies seconds. Minus one means enabled with unlimited age,
matching the source combination `ENABLE_ARCHIVE=yes` and `ARCHIVE_MAX_AGE=0`.
Rollover, reply-triggered autoarchive, staff force-archive, archive navigation,
JSON readers and report admission all recognize that setting. Unlimited archives
use PostgreSQL's positive infinity deadline. The Rust reader decodes it as no
finite deadline; `archived_at` still distinguishes an archive from an active
thread. It rejects timestamps outside Chrono's supported range rather than
performing overflowing arithmetic.

Policy changes require Read Committed isolation and hold the existing board lock
while updating retained deadlines. Public, staff and authentication roles do not
gain permission to edit board policy. The migration checks the existing transfer
and report snapshot catalogs before changing their reviewed definitions, then
reseals them. Public view columns and grants remain unchanged. Deadline updates
advance the separate HTTP modification clock through the existing trigger.

Upgrade all posting and staff instances before enabling unlimited age. Older
writers do not understand the new setting. The migration updates live historical
deadlines once; operators should account for that work on large archives.

Issue #215 remains open for the remaining lifecycle and private-data comparison.
These changes preserve the existing expiry boundary and report/audit retention
rules. They do not claim that every archive lifecycle difference is resolved.

The tests in `crates/store/tests/archive_storage.rs` cover storage beyond the old
cap, separate JSON/HTML windows, rollover clocks, disabled retention, and complete
reader-budget boundaries. `apps/public/tests/archive_storage_api.rs` exercises
both JSON listeners, ETags, the HTML limit and configured response-budget refusal.

The separate `archive_stale_statistics` regression analyzes an old board, retires
it, then reads 10,001 archives on a new board without refreshing statistics. It
checks complete JSON, the HTML window and private-board refusal using the actual
public role.

Source references: `imgboard.php` functions `archive_thread`, `trim_archive` and
`rebuild_archive_list`, and `json.php` function `generate_board_archived_json` in
the supplied original source. This is a source comparison, not a claim that the
original PHP server was executed.

The archive-age store tests cover policy changes, unlimited rollover and
autoarchive, disabled storage, deleted-row preservation, private-board refusal
and role denials. The
HTTP test covers both listeners, conditional JSON responses and unlimited HTML
visibility. Staff tests cover unlimited force-archive and its audit record.
`scripts/test-archive-age-migration.sh` exercises a populated upgrade and
dump/restore, both catalog drift refusals, unchanged public view columns and
sealed readiness. The policy
extractor test distinguishes disabled, finite and unlimited settings and checks
the captured board values.
