# Janitor queue controls

The protected request queue now has a native client for filtering, row focus,
review delays, Undo, in-place refresh and theme preferences. Ordinary forms
still work without JavaScript. Server authorization, CSRF checks, recent-session
requirements and the transactional review writer remain in force for every
submission.

## Reference and behavior

The reference is `reports/js/d8d9b0cdc33f3418/banrequests.js` from the supplied
snapshot. `fixtures/janitor-queue-filter.json` pins the complete file hash and
contains only `applySearch` and `clearSearch`. The extractor checks that fragment
against the original bytes without loading the source application.

Filtering is a case-insensitive literal substring search over the displayed
name, comment and template. It affects the loaded page, not every request in the
database. The local state suite executes the isolated source functions for 192
field/value/query combinations and compares their visibility decisions with the
native client. Reset restores all loaded rows.

The client uses the source's `dark-theme` preference and `dark-theme` CSS class.
Only theme and keyboard preferences are stored; captured posts, CSRF values and
review decisions are never saved to browser storage. Storage failures leave the
queue usable.

Supported shortcuts are Right/Left for row focus, A/D for accept/deny, C for the
amendment form, R for refresh, U for Undo and Escape to close an amendment.
Shortcuts ignore modifiers, repeated key events, composition and editable
controls. Buttons and links retain their normal keyboard behavior. The
keyboard-shortcuts checkbox disables them. Filtering also clears focus when it
hides the focused row.

Reviews wait three seconds, matching the source's delay. Undo cancels the timer
before any request is sent. The client freezes the form values at submission,
uses the existing JSON review response and refreshes the authorized queue after
success, including removal of related requests cleared by the server. It does
not infer success from a click or remove a row before the server replies.

## Safety differences

The source flushes an earlier delayed job when another job starts or the user
leaves the page. The native client allows one pending or in-flight review and
cancels an unsent timer on navigation. This avoids an unintended additional
moderation action during repeated clicks or navigation. Once a submission has
started, Undo cannot retract it.

A lost, malformed or failed response leaves the outcome uncertain. The client
does not retry the mutation and blocks further reviews until an authorized
refresh succeeds. A successful review followed by a failed refresh also leaves
stale controls blocked. The server still handles conflicting reviewers and
repeated requests independently.

New read-only refreshes cancel older reads. Generation checks prevent a late
response from replacing newer content or changing a page after navigation.
History restoration reloads the protected page rather than restoring an old
queue and its timers. The existing same-origin referrer policy and server origin
checks are unchanged.

## Verification

```sh
python3 scripts/extract-janitor-queue-filter.py /path/to/4chan-old
node --test tests/browser/janitor-queue-state.test.mjs
node tests/browser/janitor-queue-controls.mjs
RUN_JANITOR_BROWSER=1 cargo test -p board-staff --features database-tests --test janitor_requests --locked -- --nocapture
```

The nine state tests, 59 staff unit tests, strict all-target Clippy and the real
PostgreSQL/HTTP regression pass locally. The controlled Chromium suite passes at
desktop and mobile sizes, covering filtering, focus, theme persistence,
amendments, the three-second boundary, Undo, repeated clicks, lost responses,
superseded refreshes and navigation.

The database-backed browser fixture also passes all four viewport/script
combinations in CI on `988b0607`. It checks actual request creation, filtering,
cancelled review, denial, queue refresh, history and counted outcomes, while the
opaque-origin negative control still receives 403. Both browser suites run
before the full database suite in CI and remain merge prerequisites.

Local Chromium cannot launch because this workspace blocks its required Unix
socket operation, and the supported permission request fails before launch.
Browser qualification therefore comes from the hosted runner, not this local
workspace. The source-oracle browser fixture runs against a detached clone so
its CSS classes cannot contaminate the native page. The negative-control driver
waits for its refused form navigation to finish before reloading the queue.

## Remaining scope

This does not close #223. Private retained media, quick warning/global/short-ban
amendments, template autocomplete, custom menus, key remapping, thumbnail
controls, staff-history tools and source-pixel comparison remain unfinished.
The native queue keeps bounded pagination and scoped identities. The screenshots
from the database fixture are functional evidence, not a full source visual
comparison.
