# Staff force archive

The report queue's **Archive thread** form submits `action=force-archive` to
`POST /moderate`. It targets the thread OP even when the displayed report is on
a reply. The form works without JavaScript and is shown only to moderators and
higher ranks for a nondeleted, active, nonsticky thread on an archive-enabled
board. Server checks also apply to forged or stale forms.

## Authority and target checks

Force archive requires moderator rank or higher, current permission for the
board, a live session and recent authentication. Explicit board denials win over
an `all` grant. A developer flag does not raise a janitor's rank. As with existing
thread options, a permitted private board can be targeted. The existing Origin,
Fetch Metadata and CSRF checks remain in place; authentication is rechecked
after lock waits and before commit.

The board row is locked before the thread and OP. Archives are enabled when the
locked board's `archive_retention_seconds` is positive. Disabled archives,
sticky threads and already archived threads return 400. Missing boards or
threads, reply IDs, and deleted threads or OPs return 404. Invalid request values
retain the existing form-validation errors. Archive and sticky denials happen
before an unnecessary OP lock. Undead protects against automatic rollover;
it does not block an explicit force archive.

A second submission is rejected as already archived. It does not refresh the
archive lifetime or append another audit. This also applies when two valid
submissions wait for the same board lock.

## Mutation and evidence

One `clock_timestamp()` value, taken after the content locks, supplies
`archived_at`, `bumped_at` and `modified_at`. `archive_expires_at` adds the locked
board retention interval to that value. The thread becomes closed. Sticky is
already false by eligibility; perma-sage, perma-age and Undead are preserved.
Refreshing `bumped_at` corresponds to the source's archive-time `root` refresh,
not an active-thread bump. Existing public archive visibility, read-only posting
rules and conditional-read behavior apply.

The same transaction appends one `force-archive` audit with a version-1 saved OP
preimage and NULL old/new option masks. Capture requires the matching nondeleted
OP and happens before the thread update. The snapshot uses the existing
[typed saved-content contract](moderation-target-snapshots.md), including frozen
randomizer values and authorized filename metadata. It does not copy network or
session identifiers, password hashes, wordfilter payloads or media bytes.
Saved strings remain untrusted content; the audit is not a rich-HTML renderer.

Existing archive-transition triggers also run in that transaction:

- Migration 0091 [retires deletion-password hashes](archive-deletion-secrets.md)
  for the OP and all replies, including soft-deleted posts.
- Migration 0100 [retires report membership](report-group-lifetimes.md) for each
  post whose persisted illegal-report counter is complete and below three.
  Unknown, incomplete and higher-count groups survive. Retained report rows and
  audit history are unchanged.

Mutation, trigger effects and audit either commit together or roll back. Audit
failure, cancellation before commit or failed final authorization must not leave
an archived thread without its corresponding evidence.

## Source evidence and its limits

The supplied revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.
`imgboard.php:9144-9206` contains `forcearchive`; its `has_level()` call uses the
moderator default in `lib/auth.php:154`. The source checks rank, archive policy,
ID presence, query success, OP existence, prior archive state and sticky state in
that order. It calls `archive_thread`, logs action code 3 with the selected OP's
number, name, subject, comment, filename and extension, conditionally rebuilds
archive JSON, and updates the index.

The [bounded extractor](../scripts/extract-force-archive-reference.py) pins the
complete source-file hash and the exact selected function hash. Its
[13 synthetic cases](../apps/staff/tests/fixtures/force-archive.json) execute only
that function with bounded recorders for every external call. Eight cases reject
and five succeed. Cases cover rejection precedence, an Undead thread, disabled
JSON rebuilding, empty saved content and PHP integer coercion. The latter is
recorded as source behavior, not a requirement to loosen Rust form parsing.

Only control flow, recorder-call order and the action-3 payload are executed
source evidence. Authentication is a supplied boolean, rows are synthetic, and
the sticky error constant is a sentinel. `archive_thread`, `log_mod_action`, SQL,
rendering and rebuilds are not executed. No original application initialization,
includes, database connection or real user data is used. Source stored HTML is
forwarded unchanged; this does not establish byte-identical structured rewrite
snapshots or HTTP-status equivalence.

Static inspection of `imgboard.php:1746-1865` establishes the additional source
archive effects: optional OP ID regeneration, clearing email/host/pass ID and
password fields, closing the OP, clearing sticky, setting root to the current
time, updating replies, rebuilding and deleting low-illegal-report groups.
Those effects are outside the extractor's executed scope.

## Remaining differences

This implements a bounded staff action. It does not complete source archive or
staff-interface parity:

- The source extension/overlay interaction and full action-log reader are not
  reproduced by the script-free queue form and typed audit row.
- Existing [archive retention](thread-archives.md) is project policy. Fixed
  expiry, count limits and public hiding do not reproduce the source's complete
  `ARCHIVE_MAX_AGE` pruning lifecycle. Force archive does not run a new global
  expiry/count cleanup sweep; existing automatic cleanup remains separate.
- This action does not change automatic active-thread victim selection,
  protected-thread handling or the private-board rollover exemption.
- Source clearing of email, host and pass ID, OP poster-ID regeneration and
  full archive redaction are not completed here. Deletion-secret retirement
  is one specific effect. Retained content, identity/proof records, report
  history, media and backups continue under their existing policies.
- The source deletes qualifying report records and aggregates. The rewrite
  retires eligible membership while preserving history and conservatively
  retaining groups with unknown counters.
- Dynamic public reads replace source static-file rebuild calls. The oracle's
  recorded rebuild calls do not qualify generated pages or visual parity.

## Migration and qualification

Apply [0105](../migrations/0105_force_archive.sql) before starting the updated
staff binary. It widens only the audit action vocabulary and version-1 snapshot
eligibility. Historical rows and all-NULL legacy snapshots remain valid. Existing
field bounds, randomizer checks and option-mask rules remain unchanged. There
are no new grants, owners, helpers, columns, historical backfills or archive
policy changes. Existing 0091 and 0100 triggers supply the retirement effects.

Keep the forward schema and audit history during a staff-binary rollback. This
does not make pre-archive binaries safe or reverse an archive transition; the
[archive rollback constraints](thread-archives.md#migration-and-verification)
still apply.

The extractor generated and reproduced its 13-case fixture with PHP 8.4.26.
Local Linux qualification passed 463 selected Rust cases: 213 store, 126 public
and 124 staff. Three Unix-socket cases unavailable in this executor were excluded
and remain enabled in CI. Formatting and strict all-target workspace Clippy
passed. The six force-archive, seven grouped-option and eight snapshot tests also
passed both serially and with eight test threads under the CI role timeouts.

Fresh and populated 0104-to-0105 upgrades and administrator dump/restore passed,
including retained rows, constraints, grants and dependencies. TOAST dependency
identities use their owning table rather than physical OID-derived names across
restore. The checks do not discard unrelated dependencies.

Browser verification and exact-head hosted CI remain pending. The HTTP form
checks do not establish full moderator-overlay or original-page visual parity.
