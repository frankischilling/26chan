# Report eligibility and popup lifecycle

This slice follows the supplied source's board/target gates and report-window
lifecycle. The reference is `4chan-old/imgboard.php:7353-7363`,
`modes/report.php:283-305`, `forms/report.php:123-158`, and the active
`CAN_REPORT_POSTS` configuration. It does not establish complete report parity.

## Eligibility and persistence

`CAN_REPORT_POSTS` defaults to `yes`; `/j/` sets it to `no`. Migration
`0093_report_target_policy.sql` installs the corresponding
`content.boards.can_report_posts` column, defaulting to true and disabling `/j/`.
Existing board visibility and private-board row-level security still apply.
A public caller cannot use report eligibility to read a private board or post.

The GET form and POST transaction share the target check. Reporting rejects a
sticky OP and every capcoded post. An ordinary reply in a sticky thread remains
eligible. Closed threads and retained archives remain reportable while visible;
missing, deleted and no-longer-visible targets do not. There is no new blanket
closed-thread, sticky-thread or archive rejection.

POST checks eligibility before resolving its anonymous session, then checks
again inside admission. Migration 0097 rechecks retained-archive expiry using
the database clock after board, admission and session-lock waits. A previously
visible form does not reserve eligibility while those locks are contested. The report insert and
anonymous-session report activity commit or roll back together. Report rows and
session state remain private, with no added public read grants. Rendering the
GET form uses the eligible target's public board, post and thread IDs; the
subsequent advisory admission check uses the private, trusted-IP decision API.

With category mode inactive, the free-text reason is required, nonblank,
NUL-free and at most 1,000 UTF-8 bytes. An operator can import and explicitly
activate a trusted catalog to enable categorical reports; production category
rows are not supplied or invented. See [category rollout](source-report-categories.md)
for scope, selection and revision checks. CAPTCHA, Pass and complete weighted
moderation remain unqualified.

## Routes and response boundary

The native menu opens `/{board}/imgboard.php?mode=report&no={post}`. The free-text form
submits to `/{board}/report`; the categorical form uses legacy POST `mode=report`
with the query target bound to the submitted fields. GET validates the board and canonical
positive signed-64-bit post ID before querying storage; IDs stay decimal strings
in the browser, including values above JavaScript's safe-integer range.

The escaped form, success and error shells use the shared encoded-output budget
and `private, no-store`. The handler reserves and renders the success response
before attempting the write, then returns it only after commit. A failed write
cannot emit the success marker or a refreshed session cookie. Reserving output
before the transaction avoids saving a report and then failing solely because
its success shell cannot obtain output capacity. A committed write does not
prove that its response reached the browser; an interrupted response is not a
reason to replay the submission automatically.

Only renderer-marked report HTML receives the dedicated CSP. Its script source
is the exact same-origin `/static/report-popup.v1.js` path. Inline event handlers,
connections, workers, frames and framing are forbidden; forms stay same-origin.
The shell does not load the board's interactive modules. Generic legacy-route
errors do not acquire report-script permission merely by sharing a URL.

## Board themes

Forms and results with an already validated public target load the existing
`/static/theme.css?worksafe=...` stylesheet after `board.css`. The worksafe value
comes from the target's board join. The theme endpoint applies the matching
finite cookie preference and board-group default, with `private, no-store` and
`Vary: Cookie`. Styling works without JavaScript and needs no broader CSP.

Generic errors that occur before target validation omit board-style context.
They do not query private or unavailable board metadata merely for appearance.
The success shell includes its styling before admission, preserving the reserved
response boundary without an additional lookup after commit.

## Popup completion and hiding

A successful named popup sends `done-report-{post}-{board}` to its live
same-origin opener, then schedules closing after 3,000 ms. It does not use the
source's wildcard message destination. The opener accepts only its registered
window, exact origin and expected board/post payload, bound to the original DOM
post. Wrong-window, foreign-origin, generic, malformed, replayed or stale-target
messages cannot hide a post. Registrations are bounded to 16 entries and expire
after ten minutes; page teardown clears them.

Completion consumes the registration before applying hiding. Hiding is
idempotent and respects current native settings: an OP can hide its thread on a
board index when thread hiding is enabled, and a reply uses the reply-hiding
control. A thread-page OP is not hidden. Disabled features, detached or replaced
posts and suspended pages cannot gain completion authority. This browser signal
controls local presentation only; it grants no report or moderation authority.

Close and unmodified Escape do not send success. On a popup form they cancel the
window. If closing is blocked or throws, the page and its return link remain
usable. Errors do not trigger hiding or the success timer. Page teardown removes
listeners and cancels its timer. If popup opening is blocked or throws, the menu navigates to
the same canonical GET. An ordinary tab never auto-closes on success, even if it
has an opener. Escape does nothing in an ordinary tab; only its Close button
follows the validated same-origin return link. The form and return links remain
usable without JavaScript; JavaScript only adds popup completion, close controls
and opener hiding.

## Deployment and verification scope

Migration 0093 introduced this target policy. Matching services now also require
migration 0102, the [identity admission and retirement rules](source-report-admission.md),
and the [explicit category-mode rollout](source-report-categories.md).
Follow that rollout sequence before starting a paired release.
Migration 0093 preserves existing report rows and changes no categories or
grants; no new bootstrap role
is required. A binary rollback retains the additive column but can restore the
older, more permissive report-target behavior, so it is not policy-equivalent.

Store checks cover target eligibility, private visibility, policy changes during
lock waits and atomic report/activity persistence. HTTP checks cover escaped
shells, exact CSP, invalid forms and output-budget failure. Browser scenarios
cover commit-before-hiding, cancellation, ordinary tabs, popup blocking,
receiver binding, settings and idempotence. Browser behavior still requires the
current-head CI run. Exact commands and outcomes belong to the pull-request
checkpoint; these contracts do not claim full source-page or production abuse
control qualification.
