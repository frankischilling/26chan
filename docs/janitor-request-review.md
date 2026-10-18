# Janitor requests and moderator review

Issue #223 covers request creation, moderator review, staff messages and review
statistics. Work is on `feature/janitor-review-parity`. The decision model below
is implemented; the authenticated request store, queue and review endpoints are
not yet connected.

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

Creation captures the post and removes it before returning success. The native
implementation must commit capture, deletion and request creation together.
Failure must leave the post and moderation history unchanged. Captured identity,
normalized file evidence and presentation must survive ordinary deletion without
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
counts. These readers and their access tests remain to be implemented.

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
`$post`. The full caller needs independent qualification before wiring a request
event producer. The direct template-ban range producer already in main does not
establish the behavior of this separate caller.

The decision fixtures do not qualify any of these persistence, authority, media
or UI requirements. Issue #223 remains open.

## Local verification

The source extractor and PHP oracle checks pass. The full domain suite passes
348 tests, including the 2,016-case review comparison, with no ignored tests.
All-target, all-feature domain Clippy passes with warnings denied. These are
local model checks; hosted qualification and integration tests are still needed.
