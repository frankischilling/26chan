# Grouped Thread Options and sticky rank

The staff report queue links to a script-free GET/POST `/thread-options` form
for live threads. Moderators and higher ranks need current board permission;
janitors cannot use it, including janitors with a developer flag. Saving uses
the existing Origin, Fetch Metadata, CSRF, live-session and ten-minute
recent-authentication checks. Missing, deleted and reply targets are rejected;
archived threads return 400. The implementation is in
[thread_options.rs](../apps/staff/src/thread_options.rs).

## Grouped and isolated actions

Grouped submissions set all five flags together. An omitted checkbox means
false. Perma-age is the exception: accounts without Manager, Administrator or
globally scoped developer authority preserve its saved value, even if a forged
valid checkbox requests a change. The protected checkbox is hidden for those
accounts. Undead remains available to every authorized moderator.

The existing `/moderate` forms remain isolated one-flag actions. They preserve
sibling flags and reject unauthorized perma-age changes with 403. A sparse
request to the grouped route deliberately has different effects from an
isolated action. See [isolated thread options](staff-thread-options.md).

The grouped audit records the actor, board, target and effective before/after
masks in the same transaction as the update. Mask bits are Sticky 1,
Perma-sage 2, Closed 4, Perma-age 8 and Undead 16. An audit is written if and
only if those effective masks differ. A rank-only change or flag no-op has no
audit, but every accepted save refreshes modification time. Board and thread
locks serialize changes; live authority is checked again before commit.

## Parsing boundary

Only `board`, `target`, `csrf`, `sticky`, `sticky_rank`, `closed`, `permasage`,
`permaage` and `undead` are accepted in the POST body. Duplicate, unknown and
array fields, malformed URL escapes and invalid UTF-8 are rejected. Supplied
checkboxes must be exactly `0` or `1`. Rank must be one or two ASCII decimal
digits with a value from 0 through 60; omission defaults to 0. Rank is validated
even when Sticky is off, after which its effective saved value is 0. Protected
perma-age input is also validated before permission preservation.

These are intentional security differences from PHP `intval` coercion, loose
truth tests and duplicate-field replacement. Canonical flag values and omission
semantics define the supported comparison, not every malformed legacy request.
The POST body has a dedicated 4 KiB route limit.

## Rank and ordering

Sticky remains the boolean protection and admission flag. Rank only orders
active sticky threads. The rewrite uses descending Sticky, effective rank,
bump time and thread ID in list, snapshot and page-statistics queries. For an
ordinary thread, effective ordering rank is 0 even if storage contains another
value. Bump time and ID provide deterministic tie breaks; this is an explicit
rewrite mapping, not proof of the source database's tie order.

Existing rows and new threads default to rank 0. An isolated Sticky action on
an ordinary thread pins at 0; repeating Sticky on an already sticky thread
keeps its rank. Unsticky clears rank and advances bump time only on an actual
sticky-to-ordinary transition, using mutation time after locks. Other grouped
changes, including rank-only changes, preserve bump time.

Catalogs retain the original SQL position through server sorting, filtering and
hidden-row restoration. `data-bump-position` carries that bounded ordinal;
client Bump sorting and page labels use it rather than reconstructing order
from timestamps. This applies to image and text catalogs: source
`catalog.php:12-15,110` exports the original position as `b`, and
`catalog.js:1733-1737` sorts `b` ascending for `alt`. Text-only creation/reply
sorts still omit the separate sticky bucket. All-absent ordinals retain the
older-document fallback; partial, duplicate or malformed ordinals leave the
ordinary GET form in control. Image stickies keep their selected order even
when a lower-ranked sticky is pinned or matches a top filter; ordinary image
rows and text rows retain pin/filter promotion.

## Source evidence and limits

The supplied source revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.
The ignored source checkout provides these exact reference locations:

- `admin.php:3520-3526`: PHP request coercion; `3531-3549`: perma-age protection.
- `admin.php:3555-3561`: rank predicate accepts 0 through 60 and builds a
  `202701010000` timestamp prefix. The error text at 3556 and form at 3660 say
  0 through 59, despite the predicate accepting 60.
- `admin.php:3567-3580`: sticky timestamp assignment and release to `now()`;
  `3598-3620`: perma-age and Undead assignments; `3631-3641`: modification,
  write, audit call and rebuild predicate.
- `admin.php:3698-3734`: five-bit changed-mask audit and source post snapshots.
- `lib/auth.php:154-207`: rank, developer flag and global/`noboard` scope checks.
- `imgboard.php:7747`: active source threads ordered by `root DESC`.

The [bounded extractor](../scripts/extract-staff-grouped-options-reference.py)
pins source and selected-snippet hashes. Its
[2,316-case fixture](../apps/staff/tests/fixtures/staff-grouped-options.json)
executes selected PHP parsing, preservation, assignment and audit snippets,
including 2,048 manager/moderator mask transitions. Each authorization context
runs in a separate PHP process because the source rank helper caches its level.
The recorded runtime is PHP 8.4.26, not proof of all historical PHP behavior.

The fixture records prepared SQL text and synthetic audit calls. It does not
execute the whole endpoint, authentication, HTML branch, database writes,
rebuilds or timestamp conversion. Rank 60 produces `root=20270101000060`;
whether the unknown original database accepted or stored seconds 60 is
unproven. The rewrite stores 60 as a bounded integer. Neither source database
persistence nor complete source parity is established by this fixture.

## Migration and rollout

Apply [migration 0103](../migrations/0103_grouped_thread_options.sql) before
starting rank-aware public or staff binaries. Their readiness checks require
the new projection; staff also requires the audit mask columns. The additive
migration adds bounded, non-null `sticky_rank` with default 0, staff-only rank
UPDATE permission, nullable audit masks with action-specific constraints, and
rank-aware indexes. It appends the visible-thread view column while retaining
the visibility predicate, security barrier and grants. Historical isolated
audits retain NULL masks.

Older readers ignore rank, and older writers do not maintain the new rank
semantics. There is no mixed-version rank-ordering guarantee. Coordinate the
reader/writer rollout. On binary rollback, retain the additive schema, saved
ranks and grouped audit evidence; do not drop them to imitate an older schema.
No destructive down migration is provided.

## Qualification status

Local Linux qualification passed 211 store cases, followed by a final rerun of
127 selected public cases and all 107 runnable staff cases on fresh migrated
PostgreSQL clusters. Three known Unix-socket cases were excluded locally and
remain enabled in CI. Staff subprocess tests needed a local extracted-ICU
DT_RPATH; no production linker configuration changed. Formatting, strict
all-target workspace Clippy, generated-bundle checks and five pure catalog
position/priority tests passed. The 0102-to-0103 fresh/populated migration and
administrator dump/restore checks also passed with full normalized ACL evidence.

The first broad run exposed stale catalog-author assertions; they now check
nested trips and exact source badge labels. Published navigation CI separately
exposed same-actor timestamp reordering in the concurrent archive fixture.
That fixture now uses distinct synthetic actors and passed a focused rerun,
without changing production cooldown rules. Browser syntax checks passed, but
the browser-backed preference-lock test could not launch Chromium locally.
No local browser or visual pass is claimed; exact-head CI remains required.

Coverage includes:

- [Handler tests](../apps/staff/tests/grouped_thread_options.rs): grouped masks,
  sparse forms, roles, no-ops, rank-only saves, concurrent requests, strict
  parsing, authority changes during waits and transactional rollback.
- [Store tests](../crates/store/tests/sticky_rank.rs): rank constraints,
  visibility, runtime grants, ordering, pagination and statistics.
- [Upgrade qualification](../scripts/test-grouped-options-migration.sh): owned
  disposable 0102-to-0103 and fresh-schema checks, historical state/grant
  preservation, bounded ranks, grouped audit constraints and dump/restore checks.
- [Staff browser workflow](../tests/browser/staff.spec.js): grouped form and
  public ordering checks alongside existing workflows.

Full action-log presentation and the source's name, subject, comment and
filename snapshots are still absent from the rewrite audit. This grouped form
does not establish complete source UI, extension or whole-endpoint parity, or
close the other remaining work in issue #222.
