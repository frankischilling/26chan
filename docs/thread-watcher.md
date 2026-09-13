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

Native settings and controls, icon placement, draggable/fixed positioning,
mobile panel behavior, filter-driven watching and blacklist semantics remain
unfinished. Broader archive/deletion/expiry and storage-race coverage, reviewed
full watcher screenshots and passing exact-head CI are required before merge.
Production media and deployment qualification remain separate requirements.
