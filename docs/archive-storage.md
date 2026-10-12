# Archive storage and complete JSON lists

The original `trim_archive()` removes archives by age. It does not cap the number
of retained archives. `generate_board_archived_json()` selects every archived OP
in ascending post-number order. The HTML archive page has a separate 72-hour
window and a 3,000-entry limit.

The rewrite no longer uses the legacy `archive_limit` setting to delete stored
threads. That column remains for schema and configuration compatibility, but has
no effect on new OP admission. Disabling archiving hides the archive routes and
stops automatic archive cleanup; it does not erase previously stored archives.
Re-enabling it makes unexpired entries available again.

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

Board policy, entry count, byte count and rows are read in one repeatable-read
transaction. A conditional request still checks visibility before returning 304.
A matching ETag cannot expose a disabled or private board.

## Compatibility and remaining work

No data migration is needed for these changes. Upgrade every posting instance
before relying on count-independent retention: an older writer can still prune
valid archives. These changes cannot restore threads already deleted under the
old count policy. Storage expiry still uses the saved
`archive_expires_at` value; changing an operator's retention period does not yet
recompute existing deadlines. The source's independently enabled archive with an
unlimited age also needs a separate policy representation. Those remaining
lifecycle differences stay tracked in issue #215.

The tests in `crates/store/tests/archive_storage.rs` cover storage beyond the old
cap, separate JSON/HTML windows, rollover clocks, disabled retention, and complete
reader-budget boundaries. `apps/public/tests/archive_storage_api.rs` exercises
both JSON listeners, ETags, the HTML limit and configured response-budget refusal.

Source references: `imgboard.php` functions `archive_thread`, `trim_archive` and
`rebuild_archive_list`, and `json.php` function `generate_board_archived_json` in
the supplied original source. This is a source comparison, not a claim that the
original PHP server was executed.
