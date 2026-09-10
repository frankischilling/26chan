# Thread rollover and archives

Implement compatibility I-006 and the documented archive fields in I-001/I-002.
The pinned public Archive.md describes read-only threads displaced from the last
board page, retained temporarily, and an array of archived OP IDs. Boards.md
uses `is_archived`; Threads.md uses OP-only `archived` and `archived_on`.
These references are already hashed in reference-manifest.json. Exact retention,
capacity, ordering, HTML layout and all-sticky behavior are unspecified.

## Lifecycle

New threads displace the oldest nonsticky active threads in the same board-lock
transaction that creates the new OP. Order is the inverse of the active board
order: bumped_at then ID ascending. If pinned threads leave insufficient room,
reject the new thread without changing existing content. Replies and moderation
already use that board mutation lock and must remain serialized with rollover.

Add board settings `archive_retention_seconds` (0 through 2,592,000, default 0)
and `archive_limit` (1 through 1,000, default 1,000). Zero disables archives.
An enabled board archives displaced threads; a disabled board hides them through
the existing deletion model. New archive rows carry `archived_at` and
`archive_expires_at`. Displacement fixes the expiry; later policy increases do
not extend existing lifetimes. Disabling archives hides their existing entries.
The oldest archived entries exceeding the configured count are soft-deleted.

These bounds are project policy for bounded public responses, not original
per-board settings. Existing boards retain disabled archives but gain rollover.
Expiry removes public visibility at read time even without subsequent writes.
Expired/removed rows remain in restricted database storage under the existing
soft-deletion model; this change does not claim physical erasure or backup expiry.

## Authority and reads

Only the new archive timestamp columns need additional public UPDATE grants.
Do not grant public access to staff closed/sticky columns. An archived timestamp
itself makes a thread read-only regardless of the separate staff closed flag.
Prevent staff reopen/sticky actions from making an archive active again; audited
removal and reports still work while the archive is visible.

Use a `content.visible_threads` view for public single-thread, post, quote,
report and deletion lookups. It filters deleted/expired/disabled archived entries
with transaction_timestamp(), so multi-query repeatable-read representations use
one visibility instant. Active listings additionally exclude archived entries.
Do not expand staff identity, media, migration or schema authority.

## HTTP and HTML

`/{board}/archive.json` returns every visible archived OP ID as a bounded integer
array on both public and API listeners. Enabled-empty returns 200 with `[]`;
disabled/unknown returns 404. Use existing conditional response and CORS logic.
Ascending numeric order is deterministic project behavior.

Archived thread JSON sets OP-only `archived: 1`, integer `archived_on` and
`closed: 1`. Active thread JSON omits archive fields. Boards JSON advertises
`is_archived: 1` only for enabled boards. Archive expiry/removal must precede
conditional responses so an old validator cannot expose a removed entry.

Add `/{board}/archive` HTML with bounded subject/ID/time summaries and links to
the existing thread route. Show archive navigation only on enabled boards.
Archived thread pages identify read-only status and omit the reply form. Keep
password deletion/reporting functional. Public uploads remain disabled.

## Acceptance

Use actual public-role writes, owned temporary boards and synthetic posts to
verify rollover, disabled archives, sage ordering, pinned threads, cap/expiry,
no replies to archived/expired threads, author/staff removal and denied extra
grants. Cover concurrent new threads/replies and coherent response snapshots.
Test JSON/HTML/CORS/cache behavior on real data and JavaScript-disabled browser
navigation. Qualify migration from 0008 and unchanged grant boundaries, then run
workspace, browser, formatting, Clippy, workflow and hosted checks before merge.
