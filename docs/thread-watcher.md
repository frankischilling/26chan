# Thread watcher

The watcher remains under implementation in issue #88 and draft PR #89. It is
not yet a complete reproduction of either public client.

## Reference

The catalog reference is `catalog.min.1025.js`, recorded in
[the catalog manifest](public-catalog-reference.json), SHA-256
`ce645b150e747f9ad1682dc005daf30d7bc1a9f2cd473b4f1e55f99adf76fa9f`.
The thread reference is
`https://s.4cdn.org/js/extension.min.1191.js`, collected on
September 13, 2026 at 12:55:46.721 UTC: HTTP 200, 182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Both scripts were inspected as text, not executed. Browser fixtures contain
synthetic posts and watch records, not imported public user content.

## Row presentation

Both clients put positive unread counts before the board and label, inside the
thread link. Read threads have no count. `hasNewReplies` makes the link bold;
`hasYouReplies` makes it italic and supplies the title "This thread has replies
to your posts"; `archivelink` gives it 0.5 opacity. These rules were also
inspected in the pinned catalog v705 Yotsuba, Yotsuba B, Tomorrow and Photon
stylesheets. Dead links use only `deadlink`, with a strike-through, even when
older unread, archive or own-reply values remain in storage. The clients do not
append `(0)`, `(404)`, `[Archived]` or `(You)` badges.

Catalog links append `#p` only for a positive saved read position. The extension
appends `#lr` with the saved position, including zero and the dead marker -1.
An empty watch list has no generated rows.

Labels remain text nodes. The multiplication-sign removal control is a semantic
button with a descriptive accessible name and keyboard activation, styled
without native button chrome. No saved label becomes HTML or an executable URL.

## Qualification and remaining work

`tests/themes/thread-watcher.spec.js` checks normal, unread, archived, own-reply
and dead rows in all six themes, at desktop and mobile widths, on catalog and
board pages. It checks exact text, classes, computed styles, fragments, hostile
text and keyboard removal. Persisted browser tests separately exercise refresh,
cross-tab acknowledgement and own-reply tracking against the owned API. These
checks do not establish whole-panel visual parity.

Remaining native settings and controls, icon placement,
mobile panel behavior, filter-driven watching and blacklist semantics remain
unfinished. Broader archive/deletion/expiry and storage-race coverage, reviewed
full watcher screenshots and passing exact-head CI are required before merge.
Production media and deployment qualification remain separate requirements.

## Settings integration

The catalog exposes its watcher checkbox through an Options dialog with the
native `theme`, `theme-tw`, `theme-save` and `theme-close` identifiers. Saving
applies in place; enabling the watcher clears `disableAll`, as in catalog v1025.
The board/thread dialog uses the extension's Monitoring labels for watching,
automatic watching after posting and fixed positioning, plus the global disable
override. Persisted board/thread saves navigate without the fragment, as in the
extension. Other native settings categories are not implemented by this slice.

Both dialogs use the native Settings entry point rather than a separate desktop
watcher toggle. The dialog title labels only the title text, not its close
button. The Monitoring control has a stable accessible name independent of its
decorative expand/collapse indicator. Native dialog modality keeps keyboard focus out of the underlying page;
Escape and the close control discard unsaved edits and restore focus.

Changed board/thread options are merged with freshly loaded settings under the
watcher's Web Lock. Unedited options from another tab are preserved. The settings
object stays bounded to 4,096 code units. Storage or locking failures keep changes
in memory in the current tab and skip the navigation that would discard them.
No settings string is inserted as HTML, CSS or a navigation target.

The extension's fixed-position setting applies on desktop board/thread views,
with the observed initial position at left 10px, top 380px. Catalog placement
starts at left 10px, top 75px. At mobile widths up to 480px, the enabled watcher
starts hidden; the TW link shows it at the current scroll position plus 30px,
and its close control hides it without disabling watch storage.

`tests/browser/watcher-settings.spec.js` covers save/cancel, disable precedence,
fixed positioning, cross-tab draft merging, unavailable storage/writes/locks,
mobile show/hide and the unchanged no-JavaScript style page. The existing strict
CSP regression includes the exact release-owned `native-settings.v1.js` module,
with healthy alternate-script and inline-script denials. No wildcard script or
image source, new fetch permission, database grant or migration is introduced.

## Dragging and saved positions

The desktop header supports pointer dragging and saves `TW-position` in the
native coordinate format. The pinned catalog and extension Draggable routines
use percentages in the viewport interior and switch to zero-valued edge anchors
when dragged beyond an edge. Absolute positioning includes the captured scroll
offset; fixed positioning does not. Tall panels retain the reference's top-based
branch. The current navigation has no persistent top offset, so its offset is 0.

Stored CSS is not applied wholesale. A 256-character parser accepts exactly one
horizontal and one vertical coordinate, in pixels or percentages, and an optional
finite position keyword. Negative, duplicate, conflicting, calculated and other
CSS declarations are rejected. Values are bounded to 1,000,000 pixels or 10,000
percent. The fixed-position preference, not stored CSS, controls positioning mode.
Only the parsed coordinate properties are assigned through the CSS object API.
This is a security exception to the reference's arbitrary `style.cssText` restore.

Saving uses the existing lock, preserves unrelated settings and compares the
position and fixed-mode preference observed when dragging began. Cross-tab
changes, disabling, mobile transitions, pointer cancellation and page exit cancel
an active drag. Mobile placement does not overwrite desktop coordinates. The
focusable header also supports arrow keys, or Shift plus arrows for ten-pixel
steps. New gestures wait while a position save is pending; storage failures keep
the current tab usable. `watcher-position.test.mjs` covers the parser and native
geometry; `watcher-position.spec.js` covers real pointer movement, reloads, tabs,
keyboard movement, cancellation, invalid settings and unavailable storage.
