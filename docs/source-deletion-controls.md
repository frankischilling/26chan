# Full-page deletion controls

Desktop board and thread pages provide the original post checkboxes and bottom
Delete Post, File Only and Report controls. The source contracts are
`imgboard.php:2409`, `views/imgboard.php:463,599` and `js/core.js:1380-1395`.
Archived pages and catalogs have no selection form. Mobile pages keep their
existing per-post actions.

Each checkbox belongs to the bottom `delform` through an explicit HTML form
association. This avoids nesting the existing per-post forms. Native browser
submission sends checked posts in document order, followed by `mode=usrdel`,
optional `onlyimgdel=on` and the hidden password field. It works without
JavaScript. Session ownership and password authorization stay on the server.

With JavaScript, the controls reject empty selections, changed form destinations,
foreign controls and selected posts that have been removed or archived. The
20-field parser budget allows 18 whole-post selections or 17 file-only selections.
The client leaves successful controls intact, never removes posts optimistically
and never retries a deletion. A repeated submission, including after Back/Forward
cache restoration, requires a reload. A later server refusal can leave earlier
items committed; the form says so before navigation.

The core Report button opens the report page for the first selected post and does
not submit deletion. The native extension hides this desktop button, as source
`extension.js:9454` does, and keeps its per-post report actions. Disabling the
extension restores the core control.

Updater snapshots admit only the exact checkbox grammar in its own desktop post
header. Quote copies strip form controls, so copied posts cannot add selections.
The new script is permitted only by the full board-page CSP, never report or
error-page policies. No storage grants, deletion eligibility rules or public API
fields change.

## Checks

`source_post_actions` exercises the actual HTTP renderer, asset and CSP with a
role-scoped database fixture. It checks board, thread, preview, catalog, archive
and private-board boundaries. The JavaScript tests cover canonical IDs, field
budgets, snapshot grammar, form ownership, duplicate submission and page lifecycle.
Browser scenarios submit real forms with JavaScript on and off, including a later
ownership refusal after an earlier successful deletion.

Private moderation snapshot retention is a separate operator policy. These
controls do not erase audit snapshots or choose a retention period. Historical
public erasure remains governed by the bounded operator cleanup procedure.

Desktop regression baselines were reviewed from hosted run `38291301315`:
board, empty board, attachment board/thread and all six persisted themes. The
accepted changes add post selectors and the bottom controls, with the resulting
footer movement. Mobile baselines remain unchanged; an explicit visibility
assertion keeps the desktop form out of the mobile layout. These project
baselines do not by themselves establish visual parity with the original site.
