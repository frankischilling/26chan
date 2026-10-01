# Native navigation controls

The native settings panel provides persistent board navigation, a traditional
board list, scroll-based hiding, a movable page switcher and Top/Bottom arrows.
These options default to off. The existing board links and pagination remain
usable without JavaScript.

The behavioral reference is public extension v1191. `Main.setPageNav`,
`Main.setStickyNav`, `Main.initPersistentNav` and `StickyNav` establish the
controls, Shift-drag movement and scroll-direction behavior. The release and
hash are pinned in [the watcher manifest](public-watcher-assets.json). The
sixteen fixed navigation icons were collected from the official static asset
host; [the navigation manifest](public-navigation-assets.json) records each URL,
size, dimensions and SHA-256.

## Controls and preferences

`topPageNav` creates a movable copy of the current page's Previous/Next links and
one-based page label. Link parsing accepts the existing index aliases
`/{board}/` and `/{board}/0`, followed by canonical numbered pages through 999.
It rejects leading-zero alternatives, query strings and foreign boards.

`stickyNav` creates a fixed pair of Top and Bottom buttons. Holding Shift while
dragging moves either control. Shift plus an arrow key provides the same
placement function from the keyboard. The controls reuse the existing finite
position serializer, store only `TN-position` or `SN-position`, and check the
expected stored value under the shared settings lock before saving. Disabling
the feature or leaving the page cancels pending writes. Rendered positions are
clamped so stored coordinates cannot make a control unreachable. When the
persistent board bar is fixed open, movable controls stay below its measured
height; auto-hide releases that top edge so the controls can use the viewport.

`dropDownNav` keeps board selection and navigation actions at the top of the
window. `classicNav` replaces the desktop selector with ordinary board links;
mobile retains the selector. The selector retains the full directory when a
custom board list is enabled; separate links show the chosen boards. Custom
links open indexes, including from a catalog. Ordinary selection preserves the
catalog view except for `f`, which opens its index. Settings and
Edit boards open the existing dialogs. The All boards link remains available if
the directory cannot load.

With `autoHideNav`, scrolling down more than five pixels hides the persistent
bar and scrolling up shows it. Work is debounced by 50 milliseconds. A focused
control remains visible. Focused scrolling and runtime changes to the setting
reset the direction baseline, and disabling the option cancels pending hiding
work.

The local layout uses the existing board structure, theme variables and
accessible controls. It does not duplicate the original site's board categories
or claim a complete rendered-page match. The ordinary pagination currently has
Previous/Next links rather than a full numbered list; the movable switcher
preserves that same contract.

## Directory and resource limits

The controller first captures the server-rendered mobile directory. It validates
the same finite slug/title schema and retains each non-worksafe option's class.
The HTML directory shares the page content's repeatable-read transaction, so a
concurrent commit cannot mix new board labels with older content or settings.
Later DOM changes cannot enlarge that admitted set. A valid server directory
avoids another HTTP request. Pages without it retain the bounded transport below.

`GET /_watch/boards` returns an exact version 1 object with at most 100 public
board slugs and titles. It uses the existing read-only board query and response
budget. The response exposes no posting credentials, private identities or
moderation fields. It supports HEAD and ETags and is absent from the separate
public API listener.

The client allows at most 32 KiB, 4,096 stream chunks and five seconds for the
request. It requires the exact same-origin response URL, JSON content type and
strict schema, rejects redirects and sends no credentials. Empty chunks count
toward the work limit. Slugs and titles become text nodes or fixed local paths;
they never become HTML or external navigation authority.

The navigation resource imports the fixed display, position and page-navigation
helpers. The last also mounts the ordinary mobile selector and mode controls.
Icons are literal release-owned routes. Public asset tests verify the sixteen
PNG hashes, signatures and dimensions and retain the exact image CSP allowlist.
No broad image, script or worker source is added.

## Ownership and checks

The controller owns its generated nodes and one body class. It preserves the
original navigation nodes and form values, releases listeners and pending
requests on teardown, replaces an older controller if the same root is mounted
again, and recreates one set of controls after history restoration. Settings,
viewport changes and persistent-bar height changes recompute the applicable
layout.

`native-navigation.test.mjs` checks canonical pages, hostile directory fields,
finite transport work, local-only links, classic/mobile selection, scroll hiding,
bounded keyboard movement, cancelled delayed saves, and history cleanup. The
application browser tests save preferences through the actual Settings dialog,
load the real board directory, follow a selected board, move controls with the
keyboard and pointer, verify runtime 1x/2x navigation assets, and verify
cross-tab disabling. The
[combined verification record](verification-rewrite-completion.md) records the
executed checks and final CI result.
