# Thread rollover and archives

New threads displace the oldest nonsticky active threads once a board reaches
its configured `thread_limit`. A successful reply can bump a thread ahead of
another candidate; sage replies do not. The board lock serializes replies,
rollover, deletion, reports and staff mutations. If pinned threads occupy all
available capacity, a new thread returns 409 and leaves existing threads intact.

Archives are optional. An enabled board keeps displaced threads read-only until
their fixed expiry or until its archive count is exceeded. A disabled board
hides displaced threads using the existing soft-deletion model. This replaces
the earlier behavior that rejected every new thread on a full board.

## Board policy

Migration 0009 adds these operator-owned settings:

| Setting | Range and default | Behavior |
|---|---|---|
| `archive_retention_seconds` | 0 through 2,592,000; default 0 | Zero disables archives; positive values fix each newly archived thread's lifetime |
| `archive_limit` | 1 through 1,000; default 1,000 | Retain at most this many unexpired archived threads after each successful new OP |

For example, an operator may enable one day of archives and a 500-thread cap
on a deliberately selected board:

```sql
UPDATE content.boards
SET archive_retention_seconds=86400, archive_limit=500
WHERE slug='demo';
```

The public login cannot change either setting. Increasing retention does not
extend existing expiry timestamps. Disabling archives hides existing archived
threads immediately; re-enabling may expose still-unexpired entries that have
not been soft-deleted. Lowering the count cap prunes oldest entries on the next
successful new OP. Until then, reads retain the previous set, bounded to 1,000
summaries. Exact policy values, deterministic ID ordering and all-pinned handling
are project choices; the pinned reference does not specify them.

Expiry hides threads on public reads even if no new post triggers cleanup.
This is public visibility policy, not physical erasure. Base-table text and
deletion secrets remain in the database under existing grants, including the
public and staff content readers. Backups have separate retention requirements.
Archived media is not implemented; public uploads remain disabled.

## Public contract

`/{board}/archive.json` on either listener returns a JSON array of visible
archived OP IDs. Enabled-empty returns 200 with `[]`; disabled or unknown boards
return 404. Existing ETag, HEAD and API-origin CORS behavior applies. Archived
threads remain at their existing HTML and JSON URLs. The OP JSON includes
integer `archived: 1`, `archived_on` and `closed: 1`; active OPs omit archive
fields. Boards JSON includes `is_archived: 1` only when enabled.

`/{board}/archive` provides escaped HTML summaries and thread links. Navigation
appears on enabled boards. Archived threads show read-only status and no reply
form; password deletion and reporting still work. Staff cannot reopen or pin an
archived thread, but can remove it with the existing audit trail. The separate
staff closed flag remains outside public write authority.

Read-only snapshot transactions cover board policy and archive entries together.
Thread/post/quote/report/deletion lookups exclude expired or hidden entries.
Visibility is evaluated at transaction start; an already-authorized request may
finish after expiry or removal. A later conditional request must pass visibility
again before it can return 304. Previously downloaded content cannot be recalled.

## Migration and verification

Stop public and staff serving, back up the database, apply migration 0009 as the
operator, then start the matching binaries. Existing settings, text, staff flags
and deletion state are preserved, and archives start disabled. Rollover is a
behavior change even on disabled boards. Old binaries do not filter archive
state, so replacing only the binaries is not a supported rollback after archives
have been created. A rollback needs reviewed data reconciliation or restoration.

`scripts/test-archive-migration.sh` upgrades an owned database from 0008 and
checks preserved content, actual public visibility, pinned-thread constraints
and retained privilege denials. Workspace database tests cover lifecycle,
concurrent reply/rollover ordering, coherent reads and audited moderation.
`npm run test:behavior` includes a real JavaScript-disabled archive workflow.
`npm run test:archive-visual` runs six deterministic Windows layout baselines
from shared templates. These are project regressions, not original-site visual
parity evidence. See [verification](verification-thread-archives.md).
