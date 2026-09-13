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

## Pinned watcher icons and panel geometry

The 40 unchanged UI images in `public-watcher-assets.json` were captured from
public static URLs on September 13, 2026. The manifest records each URL, hash,
byte count, dimensions and collection time. Catalog v1025/CSS v705 uses image
backgrounds; extension v1191 uses image elements. Both select the 2x assets at
a device pixel ratio of at least 2 and render them in 18px boxes. The four image
families cover all six themes. No upstream JavaScript is executed.

Watch/unwatch, refresh, in-flight refresh and mobile close controls now use
these assets. The catalog leaf precedes the thumbnail instead of appearing in
its metadata row. The panel uses the observed 265px desktop maximum width,
3px padding, 17px header, theme borders and single-line ellipsis rows. The
mobile panel retains its full width and close/reopen behavior.

Controls remain semantic buttons with accessible labels, pressed/busy state
and keyboard activation. Catalog leaves become visible on keyboard focus as
well as hover; the reference's `visibility: hidden` would exclude an unfocused
leaf from keyboard navigation. Stored values cannot choose image paths. Rust
embeds a fixed asset table and CSP names each complete image URL, without an
image-directory wildcard or a filesystem-serving route.

`tests/themes/watcher-icons.spec.js` covers the six themes at 1x and 2x on
catalog/board pages at desktop/mobile widths, checks decoded image dimensions,
watch toggles, the busy/error refresh transition, close/reopen, catalog placement
and keyboard access. `tests/browser/behavior.spec.js` loads all pinned images
under the real server's CSP and checks denied-origin and unlisted-path violations;
the synthetic visual-fixture pages do not send CSP headers. `apps/public/tests/ui_assets.rs`
checks fixed bytes against both image manifests, GET/HEAD behavior, response
headers, denied writes and missing paths. Test results are recorded per commit
in the PR; adding this coverage is not itself a passing result.

This is not a full watcher parity claim. Native thread-navigation watch controls,
filter-driven watching/blacklisting, additional settings and position edge cases,
and reviewed full watcher/settings reference captures remain unfinished.

## Thread navigation controls

The captured public thread markup has mobile and desktop navigation at both
ends of the page. Extension v1191 prepends bracketed watch icons to the desktop
bars and appends a mobile button to each mobile bar. Thread pages now use those
placements instead of an icon appended to OP metadata. All four controls share
one watched state and accessible action names. The catalog leaf is unchanged.

Return, Catalog and Top/Bottom are ordinary links. Mobile Refresh follows core
v1128's full-page reload and top/bottom fragment behavior, using safe browser
APIs rather than injected meta HTML. Its server-rendered URL also works without
JavaScript. No watcher buttons are generated without JavaScript. The mobile
button gradients are unchanged pinned images served through the fixed asset
table and exact image CSP. Desktop Futaba/Burichan navigation keeps its 10px
bottom margin; the lower bar has no bottom margin. Mobile controls use the
observed 480px breakpoint, padding, rounded border and centered navigation.

`public-watcher-navigation-reference.json` records the source hashes and CSS
collection details. The work-safe mobile stylesheet was linked by the captured
thread. Its publicly available non-work-safe counterpart was separately fetched
for the warm button rules; selecting the counterpart follows the application's
explicit board work-safe flag. Full page placement and all mobile post-layout
styles are not qualified by this slice.

`tests/browser/thread-watcher.spec.js` covers synchronized top/bottom controls
in all six themes at desktop/mobile widths, fresh persisted replies through
both mobile refresh links, retained watches, and no-JavaScript navigation.
The release-image browser and Rust tests cover both added gradient assets.
Results are recorded per commit in the draft PR.

Board-page post-menu watching remains unfinished; the temporary board inline
control is not claimed as native placement. Full reference screenshots,
filter-driven watching/blacklisting and the other gaps above remain required.
