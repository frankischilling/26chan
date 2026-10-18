# Deleted media metadata

Whole-post deletion clears descriptive attachment metadata in the same transaction
as the post. Migration 0143 keeps only the post ID, upload job ID, asset ID and
public media number. Those identities remain reserved so deletion cannot reopen
an approved URL or allow a consumed upload capability to be used again.

The retained row uses `[deleted]` for the filename, one for its size and dimensions,
false for spoiler state, true for file deletion and the Unix epoch for its creation
time. These are internal tombstone values, never a replacement public attachment.
A permanent marker prevents restoring the old payload or changing its identities.
Physical outputs and their queue records still use media reconciliation and queue
retention; the tombstone does not keep those files accessible.

File-only deletion preserves the source's remaining file metadata. Archiving also
preserves content until the thread is deleted. Whole-thread deletion clears each
attachment, including a reply whose file was already deleted. Authorization or
transaction failure rolls back post and attachment erasure together.

## Existing tombstones

The migration does not sweep existing metadata. With writers and schema changes
stopped, apply all migrations shipped with the cleanup binary, then run its
bounded preview. Migration 0143 alone does not satisfy the current report and
restriction readiness checks:

```sh
board-deletion-cleanup preview-media example
```

Review the database, board, post IDs and receipt. Then apply that exact selection:

```sh
board-deletion-cleanup apply-media example --confirm-permanent-erasure RECEIPT --writers-stopped
```

Each invocation handles at most 64 attachments whose posts already have the
irreversible content-erasure marker. Run the ordinary historical post cleanup
first if old soft-deleted posts still retain their payload. Preview and apply
require the actual migration login. Changed rows or selection, catalog drift and
runtime logins are refused. Repeat with a newly reviewed preview until its post ID
list is empty; there is no automatic historical sweep.

Current public and staff readiness checks verify the new guards and narrow
privileges. The existing content catalog seal includes these changes. An
administrator restore must retain tombstones and apply outstanding deletion
cleanup before serving the restored database. WAL, backups and exported copies
need their own retention policy; these operations are not secure disk erasure.

Tests cover transactional erasure, file-only preservation, unchanged identities,
irreversibility, bounded historical receipts, permission refusals, catalog drift
and administrator dump/restore. The upgrade fixture also verifies that the
current binary refuses the intermediate 0143 schema without changing media,
then accepts the fully migrated schema with the same retained metadata. A
changed report-network helper also prevents preview until its original authority
is restored.
