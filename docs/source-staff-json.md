# Source staff JSON projections

The supplied `json.php` determines names, trips, flags and badge reply groups
separately from HTML rendering. Both JSON listeners use these rules for full
threads, index previews, catalog entries and tail replies. Saved identity fields
remain unchanged when board policy changes.

The reference is revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
`json.php` has SHA-256
`18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868`.
The extractor executes only its pure name cleanup, identity masking and
`generate_capcode_replies` blocks with synthetic values. It never loads the old
application, configuration, sessions or database.

## Identity fields

On a forced-anonymous or meta board, JSON retains a name and trip only for
the literal saved values `admin` or `admin_hl`. The source parser saves a
highlighted administrator as `admin_highlight`, so that badge is masked in
JSON. The HTML catalog checks `admin_highlight` and retains that identity.
These distinct source predicates are preserved.

Masked posts have `name: "Anonymous"` and no `trip`. Otherwise a trip-only
author has no `name` field, and nonempty names use source HTML entities.
Badged posts omit country and board flag fields. Archived thread responses
omit saved `id` labels. Tail OP metadata remains minimal and carries none of
these identity fields; its selected replies use the regular post projection.

The independent fixture has 108 identity cases and eight reply-group cases.
Its `admin_hl` and unknown label inputs test the literal source predicates.
They add no selectable badges or posting authority: the persisted badge
constraint and staff permission checks still permit only the six implemented
source staff labels.

## Meta-board policy and reply groups

Migration 0072 adds the operator-owned `content.boards.meta_board` switch.
It represents `META_BOARD`, independently of the private access switch derived
from `JANITOR_BOARD`. All 82 supplied definitions inherit `META_BOARD = no`.
The board reference now records both settings. The public readiness check
requires the new column.

When enabled, full thread, index and catalog OP objects include
`capcode_replies` only when at least one visible reply has a badge. Each key
contains numeric reply IDs in post order. `admin_highlight` joins `admin`;
the other saved labels retain their own groups. OP IDs, deleted replies and
empty groups are excluded. Index and catalog groups include replies omitted
from the five-reply preview. Tail OP metadata omits the groups, as in the source.

The existing repeatable-read snapshot includes settings, visible counts,
selected bodies and badge headers. JSON preview reads fetch only IDs and
labels for omitted replies. HTML readers do not fetch that extra batch.
The rewrite retains its existing 1,000-reply read limit. A meta-board
preview response also limits the aggregate badge header batch to 100,000 IDs,
checked before transfer. The existing serialized-response and process budgets
still apply. These finite read limits are backend resource containment.

No runtime role can change the new switch. Migration 0072 changes no privacy
policy, function, owner or grant. Private boards remain unreadable through both
public routers and direct queries by the public database role. Enabling meta
presentation does not grant access to private content.

## Verification and remaining work

The persisted HTTP fixture exercises all six badges, named and trip-only
identities, all four forced-anonymous/meta combinations, omitted and deleted
replies, empty groups, full/tail/index/catalog/archive responses and both
listeners. It verifies that projection leaves saved fields unchanged, private
access remains denied and oversized meta snapshots fail before header transfer.
The role bootstrap stages populated board, privacy-policy and ACL snapshots
before migration 0072, then compares them after the upgrade and checks runtime
mutation denials.

Reproduction commands with the owned disposable roles configured:

```text
python3 scripts/extract-staff-json-reference.py SOURCE crates/domain/tests/fixtures/staff-json.json --check
python3 scripts/extract-board-reference.py SOURCE fixtures/board-reference.json --check
cargo test -p board-domain --lib capcode --locked
cargo test -p board-public --test staff_json --all-features --locked
bash scripts/test-role-bootstrap.sh
```

The [static staff ID rules](source-staff-poster-ids.md) now govern saved labels
on new badged posts. Complete JSON parity remains under issue #216. Pass fields,
specialized media fields, remaining board metadata and legacy
navigation contracts are unfinished. Source meta-board client presentation and
posting-specific rules also remain part of the full rewrite. Execution results
are recorded in [staff JSON qualification](verification-staff-json.md).
