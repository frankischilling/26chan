# Janitor requests and moderator review

Issue #223 covers request creation, moderator review, staff messages and review
statistics. PR #295 merged the decision model and staff-message reader after all 18 PR
checks and the separate dependency advisory run passed. The authenticated request store and creation form are implemented locally.
The request queue, ordinary review endpoints and own-account statistics are
implemented locally; their full hosted qualification is pending.

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
counts. The own-account reader exposes retained history separately from those totals.

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
timezone. Hosted full-page comparisons passed on the merged PR head.

[The asset record](staff-queue-assets.json) pins the original stylesheet, arrow
images and favicon. The embedded font is served from a fixed same-origin path,
with unchanged bytes. CSP permits only those specific local images/font and the
existing media origin. The stylesheet's unused `minileaf.gif` reference remains
unavailable; no replacement artwork is invented.

## Remaining integration

The private writers revalidate current account, credential, session, CSRF,
board access and recent authentication before committing. Competing-reviewer
tests verify one restriction and one counted outcome per request. Source salts
and client-supplied unlock claims are not authority.

The review path implements amendment reasons and ordinary restriction issuance.
Private file retention and source-qualified blacklist effects remain unfinished. Its optional
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
disabled. It also checks unread counts, the strict read boundary, an empty page,
Previous navigation and reloads against four independently rendered source
states. Its local launch was blocked before navigation by the execution
environment's socket restriction, so no local pixel-parity result is claimed.
The fixture is required in hosted Linux CI.

## Hosted qualification

PR #295 passed all checks on `900d33a12ff229082d9161e2a490a76947d80ba4`
and merged as `22eb5e9d9462b30dd6afb01e420e5bcbf4bc2f3e`.
[Build run 39094770320](https://github.com/frankischilling/26chan/actions/runs/39094770320)
includes 24 exact full-page comparisons: desktop/mobile, scripts on/off,
unread/mixed/read markers, an empty page, Previous navigation and reloads.
The local browser restriction did not affect hosted qualification.

## Request-authority foundation

Migration 0206 adds request-only template settings without inventing source
operator records. Warning requests default to disallowed; the retained-post
policy stays NULL until an operator explicitly configures it. No request writer
or runtime cross-board access is installed by this migration.

The private unlock predicate locks the target board and uses only retained,
known report weights. Group-cleared reports still count; reporter-retired rows
do not. It rejects stale transaction snapshots and groups over 10,000 reports.
Missing weights deny the unlock even when the known subtotal exceeds the
threshold. Runtime roles cannot call the predicate or read the evidence table.
The restriction catalog checks the helper definition, grants and dependencies.

The source applies the ceiling after its OP multiplier. With currently qualified
weights of exactly 0.5, an OP first reaches the unlock threshold at 2,399 reports;
a reply reaches it at 2,999. The owned PostgreSQL exercise checks both boundaries,
unknown weights, group clearing, reporter retirement, invalid targets, runtime
denial, grant/definition drift, rollback and populated restore. These checks pass locally and in the first branch CI run. That run failed in
other qualification steps, described below; the complete branch is not qualified.

## Atomic request storage

Migration 0207 adds a private request store and a session-bound writer. It
captures the target's identity, normalized upload evidence and presentation
before deleting the post in the same transaction. OP requests delete the thread
only when the caller may do so; sticky OPs require an administrator. The writer
rechecks account, credential, session, CSRF and recent authentication before
returning. Actor, capacity, board and content locks serialize competing writes.

Requests have a separate 64 MiB default capacity and a 10,000-row default limit.
A snapshot may occupy at most 32 MiB. OP deletion refuses threads with more than
10,000 live posts. Missing template retention configuration fails without an
effect. The `everything` policy also fails until private file retention is
implemented; it must not silently degrade to JSON-only capture.

The owned PostgreSQL exercise passes request capture before erasure, async
warning selection, zero-day warnings, role and required-file checks, incorrect
CSRF, unsupported retention, quota rollback, outer transaction rollback, stale
authentication, revocation, removed credentials, sticky protection, real
weight-qualified cross-board requests, two competing staff connections and
populated restore. The first concurrency fixture used a sleep equal to the
statement timeout; it now stays below that deadline without increasing runtime
limits. Queue/review operations are now connected to the application; private
file retention remains unimplemented. Full hosted qualification is pending.

## Request creation form

Migration 0208 exposes a bounded form reader to the authentication role.
`/ban-request` renders scoped template choices and a related-request count; its
POST handler delegates to the atomic writer. Async `by_tpl` submissions may omit
`template` and ignore the warning checkbox, matching the source caller. A zero
`by_tpl` value falls back to the ordinary template path.

The actual router tests pass normal form reads, escaping, origin and CSRF
rejection, unknown-field rejection, async requests, zero-day warnings, repeated
submissions, related counts, expired sessions and revoked accounts. All 57 staff
unit tests and strict all-target/all-feature Clippy pass. The staff-message HTTP
exercise also passes with the current schema. The historical message upgrade
checks still pin migration 0205 before applying later migrations for optional
current-application tests.

The new creation form is functional; its layout has not been compared with the
original popup. The staff-message page's earlier pixel result does not qualify
this form. Full hosted checks of the current application changes remain pending.

## Ordinary review transactions

Migration 0209 adds atomic acceptance and denial, separate cumulative totals and
detailed review history. Its SQL decision helper matches all 2,016 executed
source cases. Acceptance retains the captured post with the issued restriction;
denial removes the pending evidence without restoring the deleted public post.
Only the primary request increments a counter. Related requests are removed
according to the accepted template and warning action. Every affected board
must remain within the reviewer's current scope.

An already-denied or otherwise missing request does not count again. Accepting
a missing request fails. Two actual staff connections competing over one request
produce one outcome. The local exercise also passes warning-only duplicate
clearing, JSON-only duplicate retention, amendment rules, changed zero-day
templates, scope refusal, quota accounting and populated restore.

Detailed outcomes are pruned when a review commits, using the source's six
calendar months. Cumulative totals are not pruned. Detailed history is bounded
to 100,000 rows and totals to 10,000 account IDs; exhausted capacity rejects the
whole review. Identity uses immutable native account IDs instead of request
cookies or a mutable username.

Review currently requires an explicitly configured `none` blacklist policy and
a supported retained-post policy. Image blacklisting and private media retention
still need implementation. The queue, HTTP adapters and statistics readers have
separate local qualification below.

## Historical cleanup qualification

The first branch CI run on `6d9e8f88` failed in the historical-erasure fixture.
Migration 0206 also fingerprints report triggers in the restriction catalog.
The fixture attested its deferred-failure trigger only in the report-profile
catalog, so the stronger preflight correctly stopped before its intended commit
rollback test.

The failure was reproduced locally with the actual cleanup binary. The fixture
now proves rejection by each catalog before attesting both inside its owned
disposable database. It then verifies the deferred rollback, removes the test
trigger and restores both original seals. Fresh/populated cleanup, unchanged
independent records, role/drift refusals and restore pass locally through
migration 0210. Production permissions and readiness checks are unchanged.

The same branch run also failed an immediate viewport-width assertion in the
public upload browser. It did not report the viewport or overflowing element.
The helper now waits up to one second for the unchanged no-overflow condition
after a resize and reports element geometry if it still fails. A browser
negative control deliberately creates overflow, requires rejection and restores
the page. Sixteen helper/verification-runner tests pass locally, including a
delayed-layout model and controls that reject persistent or incorrectly ignored
overflow. The real browser control still requires hosted qualification; no CSS
change or relaxed width threshold was made.

## Protected queue and statistics

Migration 0210 provides 25-row moderator queue pages with a one-row sentinel and
a 16 MiB capture budget. It preserves the source's all-board exclusion of
`/test/`, explicit board selection and reversed priority subset for `global1`,
`hm6` and `s6`. Stable IDs resolve otherwise unspecified timestamp ties. Readers
return only authorized boards and omit raw addresses, password/equality values
and normalized private digests.

The queue uses the original report-queue stylesheet and table classes. Saved
comments reuse the report snapshot decoder, including frozen formatting, rather
than treating captured strings as trusted HTML. The shared decoder now accepts
all six already-supported approved formats, including WebM and MP4, while
request previews keep removed public media unavailable. Its format control also
rejects unknown formats.

`/ban-requests/review` rechecks live moderator authority for ordinary approval,
denial and amendments. Blank optional length/scope fields use the source
defaults; warning amendments start with an explicit zero-day value so changing
a reason does not silently turn a warning into a ban. Repeated denial is
idempotent; repeated acceptance fails after the first review.

`/ban-requests/statistics` shows only the current account's totals, pending
requests and retained authored/reviewed outcomes. The supplied snapshot contains
statistics writers but no corresponding reader UI; this own-account view is a
native replacement contract, not a claimed copy of an unavailable page.

Local SQL tests pass both source ordering modes, test-board selection, disjoint
25/1 pagination, role boundaries, oversized-page refusal, scoped statistics and
populated restore. Accepted bans and warnings also reach the real public
posting-restriction evaluator. The actual HTTP test passes creation, saved
preview rendering, approval, denial, repeated actions, statistics, expiry and
revocation, including the warning-default UI assertion. All 59 staff unit tests,
strict Clippy and the current staff-message HTTP regression pass.

The hosted browser fixture exercises actual form creation, captured queue rows,
denial, Back navigation and cumulative statistics at desktop/mobile sizes with
scripts enabled and disabled. It receives only a synthetic session and target
IDs, never database credentials. Its twelve screenshots are functional evidence;
they do not compare these pages against source pixels. The fixture compiles and
its driver passes syntax checks locally; real browser execution remains pending.

Queue and creation-form pixels and the source's full client-side interactions
remain unqualified. Autocomplete, quick-amend shortcuts, in-place refresh,
keyboard controls and private retained media still need work. These functional
pages do not close issue #223 or establish full staff frontend parity.

## First integrated browser run

Head `1d236a2d` passed the complete Rust/database/upload step, all Linux public
browser suites, Windows visuals, media/operations, native replay, monitoring and
dependency advisories. The new janitor browser stopped at its first template
selector: the accessibility snapshot contained a combobox named `Template`, but
the exact wrapping-label selector did not match. The driver now selects that
combobox by its accessible role and name. The workflow runs this focused browser
exercise before the longer Rust suite so failures produce evidence sooner.
Fresh hosted qualification is required before merging.
