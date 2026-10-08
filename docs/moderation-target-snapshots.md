# Moderation target snapshots

The 0104 continuation adds saved-content evidence to newly written
`thread-options`, `spoiler` and `unspoiler` audit rows. A grouped effective-mask
change captures the OP; a spoiler toggle captures the selected OP or reply.
The snapshot is an immutable logical saved-content preimage, read before the mutation
under board, thread and post locks in that order. Later content edits, policy
changes or target deletion do not rewrite it.

Grouped flag no-ops and rank-only saves still create no audit. Repeated spoiler
requests still create no audit or modification-time change. The snapshot and
mutation share the existing content transaction and final live-authority check;
audit failure, cancellation or final authorization failure rolls them back
together. See [grouped options](grouped-thread-options.md) and
[image spoilers](staff-image-spoilers.md) for their existing action semantics.

The spoiler setter from migration 0078 is unchanged and remains responsible
for rejection and no-op decisions. Capture eligibility skips disabled and
private boards, missing/deleted targets and unavailable threads, and rechecks
archive expiry after the thread lock. This avoids adding post-lock waits to
denials the setter already returns promptly. It does not grant private-board
access or turn a denied setter call into an audit write.

## Saved representation

[Migration 0104](../migrations/0104_moderation_target_snapshots.sql) adds typed,
nullable columns to the existing audit table. Version 1 requires saved name,
subject, comment, comment-format stamp, staff-authorized-limits stamp,
wordfiltered marker and prior image-spoiler state. Trip, capcode, filename and
randomizer results are optional. The current bounds are:

- Name: at most 255 bytes, including an empty saved name; subject: 1,020 bytes;
  comment: 2,097,152 bytes; filename: 255 bytes.
- Trip: at most 13 bytes, matching `!` plus ten characters from `[./0-9A-Za-z]`
  or `!!` plus eleven from `[+/0-9A-Za-z]`.
- Capcode: `mod`, `admin`, `admin_highlight`, `manager`, `developer` or `founder`.
- Comment-format stamp: 0, 8-15, 24-31, 40-47, 56-63, 104-111 or 120-127.
  Staff-authorized-limits, wordfiltered and image-spoiler stamps are booleans.
- Dice result: 1-1,024 bytes; fortune text: 1-256 bytes. Both exclude control
  characters. Fortune color is `#` followed by six lowercase hexadecimal digits.
  Fortune text and color must be present together; dice and fortune cannot
  coexist. Saved results are frozen rather than regenerated.

These bounds admit historical saved content independently of today's posting
limits and board policy. Capture reads the filename through the authorized
`content.staff_post_media` metadata view, including retained metadata for a
removed attachment; it does not require the file to be available.

For wordfilter evidence, only the exact saved comment and a boolean indicating
wordfilter use are kept.
`wordfilter_payload` is post-filter data but can retain discarded URL context
absent from the final saved comment, so the payload is excluded. Search text,
hashes, network/session identifiers, secrets and media bytes are also excluded.
The actor remains the existing account ID; there are no new username grants.

This is forensic content evidence, not a self-contained safe rich-rendering
archive. Saved strings are not trusted HTML. A future reader would need its own
escaping and rendering contract, including how to handle filtered content
without the omitted payload. No history UI or equivalent of the original
action-log reader is included.

## Source evidence

The [bounded extractor](../scripts/extract-staff-action-snapshots-reference.py)
pins the supplied `admin.php` and selected `log_thread_opts_action` and
`adminToggleSpoiler` helpers at source revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Its
[48-case fixture](../apps/staff/tests/fixtures/staff-action-snapshots.json)
records the complete SQL query and every positional log argument, including
actor, board, target, masks and saved content. Cases cover OPs/replies, named,
trip-like, hostile-markup and Unicode values, changed/no-op masks, spoiler
actions 129/130 and the pre-toggle subject, including its old `SPOILER<>` prefix.

The source forwards already-stored name/subject/comment HTML and joins filename
with extension. The rewrite preserves structured saved values and a separate
spoiler boolean. The fixture does not establish identical source HTML bytes,
original database persistence, authentication, rebuilding or full action-log
parity. It uses synthetic rows and bounded SQL recorders with PHP 8.4.26.

## Rollout and qualification

Apply 0104 before starting the updated staff binary; its readiness check
requires snapshot columns. Historical audit rows and writes from older binaries
retain an all-NULL snapshot. There is no backfill: current content cannot prove
what existed at an earlier action. Version 1 is allowed only for the three
covered actions, and malformed partial or unknown-version snapshots are rejected.

Existing staff INSERT/SELECT grants cover the added columns. The migration adds
no runtime UPDATE/DELETE privilege, helper, owner, target foreign key or mask
constraint change. Retain the additive columns and captured evidence on binary
rollback; older binaries can continue to write NULL snapshots but do not capture
new evidence.

Local Linux qualification passed 213 store cases, 126 selected public cases
and 116 staff cases on fresh migrated PostgreSQL clusters. Three known
Unix-socket tests were excluded locally and remain enabled in CI. The eight
snapshot cases cover exact preimages, discarded URL-context exclusion, frozen
randomizers, lock waits, denied/private/expired targets, effective-role masks
and atomic rollback. Strict all-target workspace Clippy and formatting passed.
The 48-case source oracle's reproducibility check passed under PHP 8.4.26.

[Upgrade qualification](../scripts/test-audit-snapshot-migration.sh) passed both
populated 0103-to-0104 and fresh-schema checks, followed by administrator
dump/restore. It preserves historical NULL rows, existing data, grants,
functions, owners and dependencies. Fingerprints normalize ACL order, exclude
the restore database's name from column-grant context, and use PostgreSQL's
pretty constraint deparser to remove redundant Boolean grouping while retaining
all expressions. The complete role-bootstrap check also passed with 0104.

Initial fixture failures were corrected without relaxing production rules:
private-board posts now supply their required prepared wordfilter context, and
expired archives use ordered historical timestamps. Local staff subprocess tests
needed the executor's extracted-ICU DT_RPATH; production linker settings are
unchanged. Browser/hosted-CI qualification remains separate. This record is not
a full-parity, release or merge-readiness claim.
