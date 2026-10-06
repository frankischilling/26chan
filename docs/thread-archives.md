# Thread rollover and archives

New threads displace the oldest ordinary active threads once a board reaches
its configured `thread_limit`. Sticky and Undead threads neither consume ordinary
capacity nor qualify as victims, matching `imgboard.php:2851–2869`. A successful
reply can bump an ordinary thread ahead of another candidate; sage replies do
not. The board lock serializes replies, rollover, deletion, reports and staff
mutations, so queued posts see protection changes committed before they resume.

Archives are optional. An enabled board keeps displaced threads read-only until
their fixed expiry or until its archive count is exceeded. A disabled board
hides displaced threads using the existing soft-deletion model. This replaces
the earlier behavior that rejected every new thread on a full board.

## Board policy

Migration 0009 introduced archive retention and count settings. Migration 0086
adds the source rollover-order setting. All remain operator-owned:

| Setting | Range and default | Behavior |
|---|---|---|
| `archive_retention_seconds` | 0 through 2,592,000; default 0 | Zero disables archives; positive values fix each newly archived thread's lifetime |
| `archive_limit` | 1 through 1,000; default 1,000 | Retain at most this many unexpired archived threads after each successful new OP |
| `expire_neglected` | Boolean; default true, false on `/f/` | True chooses oldest bump clock; false chooses oldest OP number |

For example, an operator may enable one day of archives and a 500-thread cap
on a deliberately selected board:

```sql
UPDATE content.boards
SET archive_retention_seconds=86400, archive_limit=500
WHERE slug='demo';
```

Runtime logins cannot change these settings. Increasing retention does not
extend existing expiry timestamps. Disabling archives hides existing archived
threads immediately; re-enabling may expose still-unexpired entries that have
not been soft-deleted. Lowering the count cap prunes oldest entries on the next
successful new OP. Until then, reads retain the previous set. JSON reads are bounded to 1,000
IDs; the independent HTML read ceiling is 3,000 summaries. These retention/count
policies are project choices and do not reproduce the complete source archive
lifecycle. Rollover ordering follows `EXPIRE_NEGLECTED`, including the active
`/f/` override. Equal bump clocks use OP number as a deterministic local tie-break.
The source private `/j/` board bypasses trimming entirely; that exemption is not
implemented by this ordering change.

Expiry hides threads on public reads even if no new post triggers cleanup.
This is public visibility policy, not physical erasure. Base-table text and
deletion secrets remain in the database under existing grants, including the
public and staff content readers. Backups have separate retention requirements.
Media handling has separate qualification requirements; see the current media
documentation for supported formats and isolation profiles.

## Public contract

`/{board}/archive.json` on either listener returns a JSON array of visible
archived OP IDs in ascending numerical order. Enabled-empty returns 200 with `[]`; disabled or unknown boards
return 404. Existing ETag, HEAD and API-origin CORS behavior applies. Archived
threads remain at their existing HTML and JSON URLs. The OP JSON includes
integer `archived: 1`, `archived_on` and `closed: 1`; active OPs omit archive
fields. Boards JSON includes `is_archived: 1` only when enabled.

`/{board}/archive` provides a source-shaped excerpt table and thread links. Thread pages
use the native Return link to reach the board and its Archive link. Navigation
appears on enabled boards. Archived threads show read-only status and no reply
form. Public deletion is forbidden by the source deletion policy; reporting
retains its separate authorization checks. Staff cannot reopen or pin an
archived thread, but can remove it with the existing audit trail. The separate
staff closed flag remains outside public write authority.

Read-only snapshot transactions cover board policy and archive entries together.
Thread/post/quote/report/deletion lookups exclude expired or hidden entries.
Visibility is evaluated at transaction start; an already-authorized request may
finish after expiry or removal. A later conditional request must pass visibility
again before it can return 304. Previously downloaded content cannot be recalled.

The HTML listing includes visible threads whose saved bump clock is at least
the read snapshot time minus 72 hours, ordered by that clock descending.
This follows the source root ordering recorded in the [compatibility inventory](compatibility.md#archives-and-retention)
and [posting-clock notes](source-posting-times.md). Equal bump clocks use descending
IDs for deterministic local ordering; that tie-break is not source-proven. HTML
and JSON keep their distinct orderings within their existing read snapshots.

The supplied `imgboard.php:9253` uses an inclusive comparison against the root
clock for this three-day window. It is independent of storage expiry and does
not shorten the JSON archive list. HTML reads use the source 3,000-entry ceiling
from line 9264 without raising response or shared-buffer budgets. Normal stored
retention is still independently capped at 1,000; restoring the read ceiling
does not complete lifecycle parity or establish full archive-page visual parity.

### Archive rows and excerpts

The HTML page uses the source `#arc-list.flashListing` table with No., Excerpt
and View columns. Its heading gives the comma-grouped count and the three-day
window, including the source's singular and empty-list wording. View links add
the source semantic context only when it fits the supported URL grammar;
otherwise they keep the bare thread URL.

Excerpts follow `imgboard.php:9289-9327` and `truncate_comment` at line 272.
When a comment exists, a nonempty subject precedes it in bold with a colon.
A subject alone has no generated bold wrapper. The string `0` counts as empty
under the source's PHP rule. The internal `SPOILER<>` subject
prefix is removed. A submitted literal prefix stays escaped text. The projection
changes `&quot;` to an apostrophe and consecutive `<br>` tags to one space.
It counts the resulting serialized Unicode scalars before stripping tags. At
100 or fewer it keeps the safe typed markup. Longer excerpts strip every tag,
including spoilers, take 100 serialized scalars, remove an incomplete trailing
entity and append an ellipsis. The original pre-strip length check still adds
that ellipsis when removing tags leaves fewer than 100 characters.

Saved formatter stamps, filter payloads and stored dice/fortune results provide
the comment; current posting rules do not rerun those transformations. The
archive's SJIS replacement is a separate preview rule: as in the source, the
current `SJIS_TAGS` setting controls whether saved SJIS spans become `[SJIS]`
before the length check.

The source returns raw formatted HTML for short excerpts. Here, Askama renders
only typed nodes. URLs keep validated attributes rather than accepting quote
replacement inside an attribute; malformed filtered delimiters keep their
existing escaped-text fallback; fortunes use the existing safe CSS class.
These safety differences do not authorize rendering stored comments as HTML.

Before loading comment bodies, the same repeatable-read transaction totals the
selected rows' subject, comment, filter payload and dice/fortune bytes. A total
over 8 MiB fails closed. This is separate from the 3,000-row ceiling and the
final shared response budget, which still rejects oversized output without
partial HTML. Archive JSON keeps its metadata-only query and existing contract.

Four changed archive-list/empty-list baseline images still need actual CI
review. These changes do not close the broader source-parity work.

The archive-capacity database test bulk-loads an owned synthetic set beyond
normal retention to verify 1,200 entries and the exact 3,000-entry ceiling. It
checks rendered ordering, escaping and rejection under a smaller configured
response budget without returning partial HTML.

## Complete board listings

Catalog and thread-list snapshots include protected threads above the ordinary
capacity. Their independent resource ceiling is 1,000 active threads. A query
reads at most 1,001 metadata rows and fails closed if the complete list would
exceed that ceiling, before loading bodies or media. It never presents a silent
truncated catalog. Numbered pages retain their configured page policy; existing
body, badge-ID and response budgets remain in force.

Store regressions cover sticky, Undead and combined protection, archive and
soft-delete rollover, complete listings above ordinary capacity, bounded-read
failures and queued OPs that resume after protection changes. They do not prove
full original-page visual parity or the remaining private-board trim exemption.

## Migration and verification

Current binaries require migrations through 0086. For an existing pre-0085
database, apply `deploy/public-deletion-role.sql` as the bootstrap administrator
before migration 0085. Fresh databases use `deploy/roles.sql`. Stop serving, back
up the database, apply pending migrations as the operator, then start the matching
binaries. Readiness rejects a schema missing the rollover policy column.

Migration 0009 originally introduced archives. Existing settings, text, staff flags
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

`scripts/test-source-rollover-migration.sh` upgrades a populated 0085 database,
checks unchanged content, clocks, ownership, functions and privileges, and probes
policy-write rejection through seven actual runtime roles. The runtime archive
regression covers both ordering policies with disagreeing IDs/clocks, equal
clocks, protected threads, archival and soft deletion, and policy changes made
while an OP waits for the board lock.
