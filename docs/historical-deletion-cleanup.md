# Historical deletion cleanup

The source removes whole-deleted post rows and reports in
`imgboard.php:2438–2770`. Earlier rewrite migrations preserve old soft-deleted
rows rather than silently erasing an installation's history during an upgrade.
Migration 0141 adds an explicit operator path for those rows.

Installing the migration changes no historical payload or reports. Public and
staff permissions do not expand. Fresh deletion keeps its existing behavior.

## Scope

The command can remove:

- Reports whose post or parent thread is already marked deleted, including their
  private membership, anonymous activity, weight evidence and saved report capture.
  Empty report groups retire through the existing trigger.
- Payload and per-post author authority on a previously deleted post, or on a
  descendant of a previously deleted thread. It uses canonical erasure, including
  saved presentation, drawing annotations, passwords and private posting history.
- Metadata on an already-deleted thread after all its descendants are erased and
  its reports are gone.

Live posts, live threads and their reports are excluded. A file-only marker does
not make a post eligible. In particular, file-only public and staff-self deletion
continues to preserve reports.

Post/thread identity rows and attachment associations remain as irreversible
media tombstones. The command leaves independent moderation audit records,
shared sessions, quotas, Robot9000 state, global polls and other boards intact.
It does not copy deleted text into a new audit record.

## Preview and apply

Build `board-deletion-cleanup` and apply migration 0141 with writers stopped.
Use the actual `board_migrator` login through `MIGRATION_DATABASE_URL`; a runtime
login or a different login using `SET ROLE` is refused by the command.

Stop application writers and schema changes for the cleanup window. The
`--writers-stopped` argument records the operator's assertion; it does not stop
services or prove that another operator is idle.

Preview one board:

```sh
board-deletion-cleanup preview example
```

The JSON result gives the database, board, selected report/post/thread IDs and a
receipt. Preview takes locks but changes no persisted rows. Review that scope
and the irreversible consequences before applying the receipt to the same
database:

```sh
board-deletion-cleanup apply example --confirm-permanent-erasure RECEIPT --writers-stopped
```

Apply recomputes the selection while holding the board and selected row locks.
The receipt binds the selected primary rows, their saved contents, the board
and database name. A changed selection or row refuses the batch. Dependent
authority and report evidence are removed according to the scope above; the
receipt is not a separate inventory of those private child rows. It is neither
an authorization token nor a backup, and does not identify a unique server.

Each invocation selects at most 64 reports, 64 posts and 64 empty thread records.
Reports drain before their post becomes eligible. Parent metadata waits for all
descendants and reports, so one historical thread cannot turn into an unbounded
descendant update. Large histories require another preview and an explicitly
reviewed apply. The command has no automatic loop. Stop when all three ID lists
are empty.

Each statement has a ten-second limit and each lock wait a two-second limit.
The process also bounds connection and transaction waiting. Failures before
commit roll back the batch, including report cascades and snapshot accounting.
A lost response or timeout during commit can leave the outcome uncertain:
inspect the database and obtain a fresh preview before retrying. Do not treat
missing success output as proof that nothing committed.

## Database protections

The erasure guards allow explicit historical markers only for the existing
migration operator. They never undelete a row to manufacture a fresh deletion
transition. An old thread refuses erasure while it has unerased descendants.
Erased content cannot be revived or assigned new author authority.

The command checks the pinned erasure guard bodies, the existing content/transfer
catalog seal, report authority and report-snapshot readiness before selection
and before commit. It does not disable triggers, change row-security settings or grant
runtime access to private tables. Unknown schema changes require review rather
than resealing from this command. The migration itself also rejects preexisting
transfer-catalog drift.

## Physical files, audit and recovery

Public media access is revoked by the existing deletion state. Physical output
files use the separate bounded [media reconciliation](post-attachments.md#output-retention-and-cleanup)
command. Deleted attachments are eligible immediately; live attachments and
retained archives remain protected. Historical database cleanup does not unlink
files or remove consumed capability records.

Independent staff audit snapshots are private evidence, as in the source's
separate deletion log. Their retention is not tied to post cleanup. This command
does not establish a deadline for audit history, logs or backups. Operators must
set those policies before deployment and must reconcile later deletions when
restoring an older backup. Logical deletion is not secure disk erasure and cannot
recall copies already downloaded by clients.

## Qualification

The owned pre-0129 fixture covers a historical thread larger than one batch,
legacy reports and their private evidence, an old deleted reply on a live thread,
a private board, unrelated history and independent audit/session records.
The actual CLI checks preview immutability, runtime-role refusals, stale receipts,
an observed board-lock wait, catalog drift, deferred rollback, repeated batches,
snapshot accounting and irreversible tombstones. Fresh installation, populated
upgrade, unchanged migration-time data and administrator dump/restore have
separate checks.

Exact executed results belong in the PR. These checks do not establish production
backup policy, secure erasure or every remaining deletion-parity requirement.
