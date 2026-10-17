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
not qualify database writes, staff authentication or a production notice caller.
Those parts, including transfer, erasure and every public post projection, remain
under issue #220.

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

This is a private prerequisite, not a completed notice issuer. The public-post
projection, authenticated issuance transaction and post-action integration still
need implementation before the notice feature can be claimed.
