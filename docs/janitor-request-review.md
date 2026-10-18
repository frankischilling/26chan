# Janitor requests and moderator review

Issue #223 covers request creation, moderator review, staff messages and review
statistics. Work is on `feature/janitor-review-parity`. The decision model and
staff-message reader are implemented. The authenticated ban-request store,
request queue and review endpoints are not yet connected.

## Review decisions

The supplied `reports/ReportQueue.php` separates the requested warning from the
moderator's amendment. An unamended request preserves its warning flag. An
amendment discards that flag; an explicit zero-day amendment creates a warning,
minus one means indefinite, and positive amendments allow at most 1,024 days.
Amending to a zero-day template without an explicit length creates a one-day
ban. Warnings use the selected template's scope even if an amendment submits a
different scope.

An unamended, non-warning local ban becomes global when the same target already
has an active, unexpired finite local ban. Indefinite bans do not satisfy the
source query's `length > NOW()` condition. The stored scope and duration must
come from locked private history and configuration, never a form's assertion
about prior bans.

Approval clears related requests with the same private actor and the accepted
template unless that template saves JSON only. A warning clears only related
warning requests; a ban clears both kinds. Only the primary request increments
the approval count and review history. Removing its duplicates must not count
as more approvals or denials. Denial counts its own primary request once.

The caller's warning branch is retained separately from the resulting duration.
If an operator changes a template to zero days after a non-warning request was
created, the unamended caller can expire the restriction immediately while
still taking its non-warning duplicate and blacklist branches. The model does
not silently collapse these two facts.

`board_domain::ban_request` checks these decisions against 2,016 cases executed
from hash-pinned source branches. The fixture uses a fixed synthetic clock and
a supplied history predicate. It does not load source application code, SQL
adapters, credentials or operator records. The extractor appends one closing
brace to stop the duplicate-selection branch before its database query.

## Request creation and queue contracts

The source creates a request from a live, unarchived post with retained target
identity and rejects requests on `/j/`. Selected template rank and required-file
checks precede creation. An asynchronous template selection ignores the warning
checkbox; a zero-day template still creates a warning request. An explicit
warning checkbox requires the template's `can_warn` permission.

Creation captures the post and starts a deletion RPC before inserting the
request. The supplied wrapper explicitly does not wait for deletion errors, so
successful insertion does not prove the post was removed. The native writer
must commit capture, authorized deletion and request creation together. This
transactional change prevents partial moderation effects. A target that cannot
be deleted under the current staff permissions must fail without creating a
request. Captured identity, normalized file evidence and presentation must
survive ordinary deletion without
becoming public fields.

Cross-board janitor access uses the ceiling of retained report weights, with a
1.25 multiplier for OPs and a threshold of 1,500. It does not use displayed report
counts or board coefficients. The existing exact-weight evidence boundary in
[report-weight-evidence.md](report-weight-evidence.md) still applies; unknown
weights cannot be invented to unlock a request.

The all-board request queue excludes `/test/` and starts in timestamp order.
A board-filtered queue sorts by template and timestamp. Requests whose rule is
`global1`, `hm6` or `s6` are prepended in iteration order, reversing that priority
subset. Unspecified database ties need an explicit stable native tie-break.

Staff messages use 25-row pages, newest ID first, with a one-row sentinel.
Visibility depends on message board scope and staff rank. The source applies
visibility after pagination; unread counts use creation time. Review history is
pruned after six calendar months, separately from cumulative approval/denial
counts. The review-history reader remains to be implemented.

## Staff-message reader

Migration 0205 adds an empty, private operator message table and a session-bound
reader. `/messages` renders the source table structure with the supplied report
queue stylesheet; `/messages/count` returns an unread count without moving the
read marker. The browser marker is a bounded decimal timestamp, separate from
the authentication-cookie parser. Duplicate or malformed markers reset it to zero.
It grants no access. Reading a page updates a host-only, HttpOnly, SameSite=Strict
cookie; production also requires Secure.

The database checks the live account, credential, session and CSRF proof both
before reading and before returning. Janitors see global messages and messages
matching an allowed board; moderator-and-above ranks see all messages, as in the
source. Runtime roles cannot read or write the table directly. The authentication
role can execute only the scoped reader, and startup checks require that split.
The existing restriction catalog detects definition and grant changes.

Messages are limited to 16,384 bytes and 128 board entries. Reads reject an
operator inventory larger than 10,000 messages. Pages preserve the source's
select-before-filter behavior, including removal of the last visible item when
the raw page has a sentinel. The read divider appears only before the first
message strictly older than the previous marker.

Operator text is escaped and line breaks are preserved. The source's arbitrary
HTML and external image embedding are not copied; trusted private image
presentation still needs a native path. Dates currently use UTC with the source
`m/d/y H:i` format. The supplied reader does not establish its deployment's PHP
timezone. Full rendered-page comparisons remain outstanding.

[The asset record](staff-queue-assets.json) pins the original stylesheet, arrow
images and favicon. The embedded font is served from a fixed same-origin path,
with unchanged bytes. CSP permits only those specific local images/font and the
existing media origin. The stylesheet's unused `minileaf.gif` reference remains
unavailable; no replacement artwork is invented.

## Remaining integration

The private writer must revalidate current account, credential, session, CSRF,
board access and recent authentication before committing. Conflicting reviewers
must not issue two restrictions or count one request twice. Source salts and
client-supplied unlock claims are not authority.

The review path also needs amendment reasons, retained file policy, exact
restriction issuance and source-qualified blacklist effects. Its optional
delete-all call is commented out in the supplied source. No such effect should
be inferred from a template label.

The source request-range caller reads `$row['email']` after capturing the post in
`$post`. The complete isolated caller now runs against 97 cases covering warning
selection, asynchronous templates, mobile metadata and local/global rules. The
unchanged caller produces no range events. A negative control changes only that
variable to `$post` and produces one mobile global-rule event, confirming that
the fixture can observe the branch. The native request writer must not silently
add that event. The direct template-ban producer in main is a separate caller.

These cases use synthetic post and template records, intercepted SQL and a
delete-dispatch recorder. They establish caller decisions, not SQL correctness,
successful deletion, retained media or authenticated access.

The decision fixtures do not qualify any of these persistence, authority, media
or UI requirements. Issue #223 remains open.

## Local verification

The source extractor and PHP oracle checks pass. The full domain suite passes
349 tests, including the 2,016-case review comparison and 97-case request caller, with no ignored tests.
All-target, all-feature domain Clippy passes with warnings denied. These are
local model checks; hosted qualification and integration tests are still needed.

The owned PostgreSQL 16 message exercise also passes migration rollback,
preserved configuration, source pagination, board visibility, real auth-role
reads, wrong-session/CSRF rejection, private-table denial, revoked-account
rejection, grant-drift detection and populated dump/restore. The actual router
test passes escaped output, unread cookies/counts, moderator visibility, expired
sessions, removed credentials, strict query fields and fixed asset routes.
The first local run caught an ambiguous SQL variable; the first parser test
caught reuse of the fixed-length authentication-cookie parser for a timestamp.
Both defects were corrected before these checks passed.

All 56 staff unit tests and strict all-target, all-feature staff Clippy pass.
The complete-page browser fixture compares actual staff HTTP output with the
independent source view at desktop/mobile widths, with JavaScript enabled and
disabled. Its local launch was blocked before navigation by the execution
environment's socket restriction, so no local pixel-parity result is claimed.
The fixture is required in hosted Linux CI.
