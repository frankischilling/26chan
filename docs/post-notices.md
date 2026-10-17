# Staff-issued post notices

The notice-selection model matches 1,440 cases executed from the supplied
`admin.php:2901–2922` block. The warning and ban labels come from
`config/global_strings.ini:378–379`.

A selected notice uses the default warning or ban text unless a manager or a
staff member with the global `banmsg` flag supplies nonempty custom text. An
unprivileged custom field is ignored. Custom text is HTML-escaped; it is not
parsed as comment markup or passed through board wordfilters. Native text is
bounded to 4,096 bytes and rejects NUL. These are explicit input limits.

The source appends each notice after the stored comment, which already contains
dice, fortune and drawing annotations. Repeated issuance appends another notice.
The asynchronous template path disables notices before this block; it must not
inherit a new manual-form option accidentally.

For ordinary manual issuance, the source also updates the thread timestamp and
marks the target's reports cleared. It skips that clear when the selected action
is delete-post, move or archive. This is `admin_clear_reports`, a separate
unweighted operation labelled `Auto-clear`; the weighted moderator group-clear
function and its report-challenge history are not substitutes.

The extractor runs only the pinned block with recording stubs. Its corpus
qualifies selection, escaped HTML and the update/auto-clear conditions. It does
not qualify database writes or staff authentication. Separate integration tests
exercise the authenticated manual issuer and persisted notice lifecycle below.

## Private automatic report clear

Migration 0202 adds a private, unweighted clear operation for the restriction
issuer. Runtime database logins cannot call it. It locks one board, the shared
report gate, then the current group and target reports; it never creates a new
group lifetime from retained history. More than 10,000 target reports are refused
before updates.

Automatic clear has its own timestamp. Existing manual clear timestamps and
actors remain unchanged. The staff queue displays `Auto-clear` at group level;
per-report history keeps the actual moderator attribution. Reporter-retired rows
are excluded, matching rows deleted by the source's reporter-wide purge.

Later reports inherit an active retained automatic clear, including legacy
reports without a group row. Their normal report event and a new clear-history
event are recorded at admission. The initial automatic clear records neither.
The source's actor lookup uses `LIMIT 1` without ordering; the native path picks
the oldest eligible report deterministically. It never substitutes the aggregate
label for that report's actor. Manual clearing an automatically cleared target
returns without reweighting it or recording another manual clear.

## Manual issuance and saved notices

Migration 0203 stores an ordered notice array separately from the immutable
comment body. Existing posts receive an empty array. A checked manual ban-form
option appends one notice, advances the thread modification clock and performs
the private automatic report clear in the same transaction as the restriction
and its audit. The option starts unchecked. The asynchronous template issuer
continues to disable notices.

Both the form handler and database enforce custom-message authority. Global
`banmsg` access permits custom text; a board-scoped flag alone does not. The
database repeats the staff-session, board-access and recent-authentication
checks. It also bounds each post to 16 notices of at most 4,096 bytes each. A
capacity refusal rolls back the restriction, audit, counters and display change.
Unbanning does not remove a previously issued notice.

Thread and board pages, JSON, catalogs, search, archives, RSS and homepage
excerpts include notices after the saved comment annotations. Text remains
escaped and repeated notices remain separate. New report snapshots preserve the
array; old snapshots without the field read as an empty array. Thread transfers
include notices in their sealed byte budget, source hash and destination copy.
Content erasure clears the array and prevents restoring it on a tombstone.

Local tests cover custom-message permissions, default warning/ban labels,
repeated issuance, count-limit rollback, report auto-clear, transfer, erasure,
homepage excerpts and escaped JSON/RSS/catalog output. The migration checks
populated upgrade, rollback, preserved historical rows, unexpected default
function grants, seal drift and dump/restore. Hosted CI must still qualify each
published commit before merge.

Issue #220 remains open. Combined post-ban delete, move and archive actions and
the other outstanding restriction/status workflows still need their own complete
source-matched integration. The manual endpoint has no combined post action;
future callers must preserve the source's auto-clear exclusions.

## Browser bundle qualification

The initial notice change made the generated worker 262,310 bytes, above its
unchanged 262,144-byte budget. The build now also minifies parse5's directly
called `_insertCharacters` and `_appendCommentNode` methods. The existing
source checks reject quoted references or use outside parse5. Public exports,
application properties, parsing checks and the size limit are unchanged.
The regenerated worker is 262,086 bytes. All generated-file checks and 68
focused filter, updater, auto-watch, semantic-reply and flag tests pass locally.

The subsequent media/operations run found a historical erasure fixture that
still expected pre-notice post rows. Its saved rows now gain only the empty
notice array introduced by migration 0203. The test also seeds a live notice,
requires deletion to erase it, and rejects restoring it on a tombstone.
Controls confirm that invented notices and changes to older historical fields
still fail preservation. The failure reproduces before the fixture correction;
the complete local upgrade/erasure/restore sequence passes afterward. Runtime
erasure code and permissions are unchanged.

The broader restriction catalog also covers triggers attached to threads. The
autoarchive rollback test's deliberate fault trigger therefore caused an earlier
`P134R` readiness refusal instead of reaching its intended `ZX001` archive failure.
The local diagnostic reproduced that distinction. The test now verifies intact
readiness, installs its owned trigger transactionally, confirms that the changed
schema invalidates the old integrity record, and records only that temporary
test schema. Removing the trigger must restore the original catalog hash exactly.
The OP-bump concurrency witness uses the same test-only procedure. Production
catalog definitions and runtime permissions are unchanged.

Both corrected database tests pass locally:

    cargo test -p board-store --all-features --locked --test autoarchive -- --nocapture
    cargo test -p board-public --all-features --locked --test op_bumps -- --nocapture
    cargo clippy -p board-store -p board-public --all-targets --all-features --locked -- -D warnings

Formatting and diff checks also pass. These focused results do not replace the
full hosted run on the changed commit.

## Independent board-read budgets

Board snapshots measure saved comment bodies and notice arrays separately.
Unused notice capacity cannot enlarge the ordinary 64,000-byte-per-slot body
allowance. Only selected posts with a persisted staff proof add the existing
larger body allowance. Notices have their own bounded allowance for PostgreSQL
array encoding; the combined read still obeys the previous overall ceiling for
1,000 selected threads and their requested reply slots.

The earlier implementation added notice capacity to the general body budget,
which admitted an expanded ordinary comment that the existing wordfilter test
requires to reject. That failure reproduces locally. The corrected four-test
wordfilter suite passes, including maximum-sized escaped notices on a small
post, continued rejection of an oversized saved body with and without notices,
and metadata-only reads that transfer no post bodies. Unit checks preserve the
same total ceiling for both ordinary and authorized posts.

The homepage migration test now separates the historical 0155/0156 schema from
the current serving schema. It checks that the current reader rejects the old
notice-free view, applies the remaining migrations, and compares retained rows
with only the reviewed CAPTCHA-policy defaults and empty notice array added.
Historical and current administrator dump/restore checks both pass, including
current readiness and public projections. The production readiness query is
unchanged.

After the budget correction, all 422 store tests pass with every feature enabled
on a fresh owned PostgreSQL 16 database, with no skipped tests. The actual
migration binary applied the full schema before that run. Store all-target,
all-feature Clippy also passes with warnings denied. The homepage migration
script passes separately against its own historical and current databases.
