# Full-page deletion controls

Desktop board and thread pages provide the original post checkboxes and bottom
Delete Post, File Only and Report controls. The source contracts are
`imgboard.php:2409`, `views/imgboard.php:463,599` and `js/core.js:1380-1395`.
Archived pages and catalogs have no selection form. Mobile pages keep their
existing per-post actions.

Each checkbox belongs to the bottom `delform` through an explicit HTML form
association. This avoids nesting the existing per-post forms. Native browser
submission sends checked posts in document order, followed by `mode=usrdel`,
the thread-only `res` context, optional `onlyimgdel=on` and the hidden password
field. Board indexes omit `res`. It works without
JavaScript. Session ownership and password authorization stay on the server.

With JavaScript, the controls reject empty selections, changed form destinations,
foreign controls and selected posts that have been removed or archived. The
20-field parser budget allows 18 whole-post selections or 17 file-only selections
on an index, and 17 or 16 respectively on a thread page. The thread return control
must remain attached to its original form and match the displayed thread.
The client leaves successful controls intact, never removes posts optimistically
and never retries a deletion. A repeated submission, including after Back/Forward
cache restoration, requires a reload. A later server refusal can leave earlier
items committed; the form says so before navigation.

After a successful thread-page deletion, the server resolves `res` against public
post visibility. A surviving OP or reply returns a 301 Location and zero-delay
meta refresh to its canonical thread and post anchor. A removed or unavailable
return target instead shows the source "Updating index..." page, which refreshes
to the board after two seconds. Index-page deletion uses that page directly.
A single already-missing selected post takes the same index path before return
lookup. Errors retain their status and do not redirect, including a later refusal
in a partially committed batch.

These rules follow `imgboard.php:2438-2452,6883-6936,7593-7601`. The return ID is
validated before mutations as a canonical positive signed 64-bit decimal; malformed,
duplicate and overflowing values fail without deleting anything. Targets stay on
the local origin and authenticated deletion feedback uses `private, no-store`,
rather than the source's Referer-derived scheme and short shared cache.
The native per-post client omits `res` and recognizes the entire fixed index
response without rendering or executing its HTML. It still refuses HTTP redirects
and never follows a meta refresh itself.

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
