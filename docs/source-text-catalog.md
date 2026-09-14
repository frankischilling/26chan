# Text-only catalog tables

Text-only boards render the supplied source's subject/reply/date table. The
source is `catalog.php:59-64,104`, `js/catalog.js:2029-2110` and the six catalog
styles below, inspected on 2026-09-14. PHP and JavaScript were read, not executed.
The table fixtures contain synthetic subjects and counters, not copied posts.

| Source path under `4chan-old` | SHA-256 |
|---|---|
| `catalog.php` | `9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946` |
| `js/catalog.js` | `12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e` |
| `css/catalog_yotsuba_new.css` | `e1220c4d9428f6ca972ce9cbeb2189bc54042eb078cd63efab45908214d3c931` |
| `css/catalog_yotsuba_b_new.css` | `97c245edbe8c2bfc904812439c1864c597bf3090a3334e75c04317d57a5eea1b` |
| `css/catalog_futaba_new.css` | `b8138f14625c06e50734296c50ab4057569cff3c8ff811428cabcbcb25e903dc` |
| `css/catalog_burichan_new.css` | `f071dc2a691cd6e32aa18409603a37c62c5584de2e3ef2bd34d79b17c0e96f6a` |
| `css/catalog_tomorrow.css` | `e270720e22e36ff13b67323086922be2ecef4f4ef07a3fe29881c070c9ceaca9` |
| `css/catalog_photon.css` | `09fd2921f41cedc9cdf6d78264cc53c45935d0a71c7585a5d46e7a8c2e572df5` |

## Rendering and controls

The `txt-no` link is `»`; `txt-sub` contains only the escaped subject link.
`txt-rep` holds the visible reply count, italic when the existing source bump
predicate applies. `txt-date` uses the OP's `now` spelling in America/New_York,
including seconds. `txt-ctrl` holds the release-owned thread menu. Subject and
comment search still use the same prepared teaser as image catalogs, but the
teaser stays in an inert template inside the subject cell. Search metadata is
escaped attribute text, never executable markup.

Rows have no thumbnail, inline teaser, image counter, sticky/closed icon, watch
button or pin page number. Image-size, teaser and image-spoiler controls are
hidden without changing the bounded stored preference format. The source text
renderer has no separate sticky-first branch: all four sorts use their normal
rank, with pins first in the browser. Pin reply deltas retain the source's
` (+N)` versus `(+0)` spelling and baseline update. The first cell of a pinned
row has the black one-pixel left shadow.

The table is centered at 80% width, with separate 2px border spacing and 4px
vertical/2px horizontal cell padding. Cell widths are 20/100/200/25px for the
link/reply/date/menu columns. Source cells use content-box sizing; Futaba and
Burichan inherit 12pt serif text while the other four styles set 10pt cells.
At widths of 480px or less the table is full width and date/menu columns are
hidden. The subject remains a native link, including when JavaScript is off.

Excluded GET results are serialized inside a template's table/tbody. The live
script moves the existing rows into the active tbody; it never rebuilds user
text as HTML. Empty-result messages stay outside the table, retaining the
existing semantic GET fallback instead of relying on the source's invalid
div-inside-tbody foster parenting. Menu actions retain the existing finite
DOM builder, checked report URL, keyboard focus and bounded pin/hide storage.

## Verification and remaining work

`catalog_controls.rs` checks persisted sorting with and without sticky priority,
escaped table fields, bump italics, source dates against JSON, inert teasers and
GET exclusion. `tests/browser/text-catalog.spec.js` posts to a real text-only
board and checks no-JavaScript HTML, GET-excluded row restoration, local sorts,
empty-result recovery and the exact 480/481px breakpoint. The existing persisted
teaser browser case retains its text-only LF/search assertion in the inert
template.

`tests/themes/text-catalog.spec.js` checks six themes on desktop and mobile and
exercises release search, sorting, pin/hide/report menus, row identity and valid
tbody children. Twelve new Windows Chromium table captures and one menu capture were reviewed for column
alignment, readable dates, italic counts, literal escaped subjects and mobile
column removal. The initial run exposed a header-inclusive test selector and a
CSS custom-property inheritance mistake; both were corrected before accepting
the captures. Browser, viewport, timezone and font environment follow the pinned
theme suite. These are rewrite regression images backed by source declarations,
not screenshots of an executed legacy site.

This implements issue #159's table slice and advances #82, V-005 and V-009.
Source date-hover previews are covered by the [hover slice](source-catalog-previews.md),
pending its own qualification. Complete surrounding catalog-page layout, native
theme/highlight options and remaining link/filename/encoding differences remain
unfinished. This is not full text-catalog or full-site parity. No database
migration, dependency, runtime grant or media/staff boundary changes are needed.
Deploy the public binary to serve the new templates and fixed assets.
