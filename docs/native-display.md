# Local dates and custom board navigation

The native settings panel now provides local post dates and a custom board list.
Local dates are enabled by default. The board list is optional, persists through
the existing locked settings writer, and can be edited without reloading the
page. Core navigation and server-rendered dates remain usable without JavaScript.

## Reference and adaptations

The permitted public extension v1191 is pinned in
[the watcher asset manifest](public-watcher-assets.json), SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its `Parser.getLocaleDate` formats local time as `MM/DD/YY(Day)HH:mm:ss`.
`Parser.init` computes the time-zone tooltip from the current browser offset;
`Config` enables local time by default. `CustomMenu` provides ordered board links,
an editor, persistence and a temporary Show all control. These facts come from
released client code; they do not depend on the excluded server checkout.

The desktop header uses its HTML `datetime` value; the mobile header uses its
canonical `data-utc` seconds. The controller preserves those values and
the server API's timestamp and `now` fields. Dates follow the browser's time zone,
including daylight-saving transitions and fractional-hour offsets. The tooltip
uses the offset at module initialization, as the public client does.

Board preferences are normalized to lowercase alphanumeric slugs and produce
same-origin paths. Repeated separators are accepted. This is a deliberate bounded
adaptation of the public client's token loop and domain mapping. Board names are
not fetched or verified while editing; an unavailable board uses the ordinary
404 route. The project retains its own navigation layout and accessible dialog
controls. Complete original navigation, desktop/mobile geometry and settings
parity remain outside this slice.

## Ownership and limits

The list accepts at most 1,024 UTF-8 bytes and 64 entries, each at most ten ASCII
letters or digits. Stored text never becomes HTML, script, CSS or a foreign URL.
At most two existing board menus receive custom navigation. Show all restores the
original menus for the current page without erasing the saved list. Ordinary
mobile and persistent drop-down selectors retain the full directory. Custom
links open indexes even on catalog pages. The
[whole released menu replay](public-page-menu-reference.json) checks twelve
owned index/catalog and desktop/mobile/classic/drop-down cases. Complete menu
styling and editor placement remain unqualified. Disabling the
extension restores the original navigation and dates.

The date controller tracks at most 10,000 date elements and scans at most 40,000
DOM elements per refresh. Mutation work is coalesced. The controller owns only
the text and tooltip it changed, releases removed elements, and restores owned
state on page suspension or destruction. In the mobile header it changes only
the date's first text node, keeping both post-number links. Changes to the
timestamp or child ownership release that annotation. A restored browser-history page
reattaches the controller.

The shared quote projection retains the server text behind localized dates.
Local quote previews and inline copies read that original text before applying
their display setting. Date annotations never expand the accepted worker tree
or acquire permission to load resources. The page receives one additional fixed
JavaScript path; its network, worker and media policies are unchanged.

Editor saves merge only their own fields under the existing Web Lock. Other
tabs receive normal storage events. When storage or Web Locks are unavailable,
the settings remain editable in the current tab. Cancel leaves the saved list
untouched. Saving the dedicated board-list editor applies immediately, including
when opened from the settings dialog.

## Verification

`native-display.test.mjs` checks finite preference parsing, invalid timestamps,
DST boundaries, UTC and Nepal offsets, original quote text, copied-date
restoration, navigation order, hostile input, editor cancellation and bounds,
and page lifecycle cleanup. It intercepts synthetic browser resources and
rejects unexpected requests.

`native-display.spec.js` uses the actual application to check persisted editing,
cross-tab changes, mobile navigation, storage denial, local-time settings and
unchanged server-rendered/API dates. The existing quote, backlink and inline
suites also exercise the shared projection after this change.

```text
npm run test:display-core
npm run test:display
```

Current validation and any remaining failures are recorded in
[rewrite completion verification](verification-rewrite-completion.md).
