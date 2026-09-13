# Catalog pin and hide behavior

The public catalog client `catalog.min.1025.js` and the six catalog v705 theme
stylesheets define the observed interaction contract. The JavaScript observation
uses SHA256 `ce645b150e747f9ad1682dc005daf30d7bc1a9f2cd473b4f1e55f99adf76fa9f`.
The upstream client is inspected as text, never executed by the application.

Pinning groups sticky threads first, other pinned threads second, and ordinary
threads last. Each group retains the selected sort. A pin remembers its last
seen reply count; a positive rendered delta advances that count. Original board
page positions are one-based and use the coherent catalog bump ordering and the
board's configured threads-per-page value, independent of display sort or search.

Normal catalog views exclude hidden threads. Search can surface them. The
hidden-only view takes precedence over search, and unhiding its final thread
returns to the ordinary view. Hiding a search result removes it immediately;
a later search or display update can surface it again, matching the reference.

Board-local storage uses `4chan-pin-<board>` and `4chan-hide-t-<board>`. Exact
decimal thread IDs are retained as strings. Pin values are nonnegative safe
integer reply counts; hidden values are true. Missing IDs older than the newest
catalog entry are pruned, while newer absent IDs survive catalog refreshes.

## Bounded local behavior

Each stored object is limited to 65,536 code units and 1,024 entries. Invalid
objects are discarded, invalid entries are omitted, and prototype-shaped keys
cannot become thread IDs. Adding at the entry cap evicts an absent entry first,
otherwise the first stored entry. Storage failures leave the current tab usable.
These input bounds are local containment, not behavior claimed of the reference.

Menus use owned DOM nodes, theme variables, and no HTML-string insertion or
additional network requests. Alt-click pins, Shift-click hides, and a thumbnail
context click opens its menu. Menus also support focus, arrow keys, Home/End,
Escape, and Tab. Their visibility on keyboard focus and touch devices is a local
accessibility extension. The menu stays within its catalog card rather than using
the upstream body's pixel-positioned popup. Unpin-all is an explicit control.

The menu's Unhide action always removes hidden state, including during a search.
This intentionally corrects the reference's mismatch between that menu label and
its mode-dependent hide handler. An all-hidden catalog has a reversible empty
message. Reset exits the hidden-only view but preserves pins and hidden entries.
Report links lead to the existing thread report form; no new reporting endpoint
or watcher implementation is implied.

## Verification scope

Browser regressions cover ordering, exact IDs, original pages, search precedence,
hidden view transitions, reply deltas, board isolation, storage validation and
failure, menu focus, gestures, and all-hidden recovery. Six theme cases compare
the observed menu colors and typography, test mobile bounds and pin borders, and
capture new menu-only regression screenshots. Existing ordinary catalog baseline
images are not replaced to accommodate the new controls.

Whole-catalog visual parity, generated-teaser search-field parity, watch lists,
other settings, and production media qualification remain unfinished requirements.

The real-thumbnail regression combines the attachment and text-only catalogs and
asserts that spoiler, deleted-file, and no-file classes are actually present.
Every card is pinned and unpinned on desktop and mobile while preserving its
original DOM node, image source, and explicit dimensions. This prevents ordinary
image-only fixtures from masking missing pin styling on placeholder thumbnails.
