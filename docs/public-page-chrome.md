# Public page navigation, titles and footer

The board, catalog, thread and archive templates render the real local board
directory above the page and in the footer. The mobile selector sorts the same
directory and keeps the catalog view when moving between boards, except for
`f`, whose destination is its index. Settings links have a server theme-page
fallback; board and thread scripts bind those existing links to the native
dialog. Archive pages load only the fixed navigation module.

## Public reference and limits

The sanitized [DOM observations](public-page-chrome-dom.json) retain only
navigation, banner-title and footer structure from the public board and catalog
pages. The response bodies, posts, media, advertising, inline handlers and
disclaimer text are not retained. The original server response has no footer
navigation: the released Core clones the desktop navigation and changes the
footer's IDs.

The [component record](public-page-chrome-reference.json) comes from the whole,
unchanged public Core v1128 and pinned v716/v705 stylesheets on owned empty
pages. Its normal navigation functions build the mobile list, clone the footer
and select destinations. Six real selection/reload cases cover index/catalog
destinations and `f`. Twelve stylesheet selections use the unchanged Core
initializer and prove that changing a board's desktop theme keeps its mobile
stylesheet. Setting the literal mobile-disable preference removes that link.

Board mobile CSS follows the safety-specific server link. Catalog fixtures
include the theme class added by the public v1025 catalog loader. The record
covers six themes, board and catalog modes, both safety settings, widths 390,
480, 481 and 1280, and display densities 1 and 2. It compares 13 components,
including 24 computed properties, page margins, banner spacing, title typography
and footer styles. Visible navigation links also compare four hover properties.
This qualifies those inputs and components. Advertising, banner image loading,
page-wide original startup and complete original-page placement are outside
the replay. Global Search and the original information/footer destinations
still need implementation; the empty banner container supplies no image.

Identical computed style objects share a hash-keyed dictionary in the record.
All 192 cases retain every expected property; the application test validates the
keys and expands them before comparison. Expansion was checked against the full
unpacked record, then against the original client and application again.

```text
node scripts/verify-public-page-chrome-reference.mjs <reference-directory>
```

Every input asset's size and SHA-256 must match the pinned record. All requests
outside the owned fixture are aborted. The fixture hostname replaces only the
released navigation function's hostname mapping.

The [menu record](public-page-menu-reference.json) executes the whole unchanged
Core v1128 and extension v1191 on owned navigation shells. Twelve index/catalog
and desktop/mobile/classic/drop-down cases prove the full sorted selector,
custom index links, two restored original lists, and the released nonclassic
desktop drop-down branch. Its hostname mapping is the only function replaced.
This replay calls the released menu and persistent-navigation functions; it
does not reproduce complete extension startup, menu pixels or editor placement.

```text
node scripts/verify-public-page-menu-reference.mjs <reference-directory>
```

The [mobile dark record](public-page-dark-reference.json) loads the same whole
clients and calls the released `Main.addCSS`, then supplies its admitted
`m-dark` class. Forty-eight cases cover six themes, both safety settings, widths
390/480 and densities 1/2. They compare six color, background and bottom-border
properties on the same thirteen components, plus visible navigation/footer-link
hover colors. Catalog dark mode, native preference transitions, full extension
startup and whole-page pixels are outside this record.

```text
node scripts/verify-public-page-dark-reference.mjs <reference-directory>
```

## Local behavior and authority

Directory rendering is capped at 100 boards and retains the current board when
it lies beyond that set. Template escaping applies to titles and descriptions.
The mobile controller captures at most 100 admitted local slugs and six mode
controls. It does not fetch a directory or admit external destinations.
Persistent navigation reuses that validated server directory and preserves its
non-worksafe classes. The bounded HTTP directory remains a fallback for pages
without the server selector. Custom links keep their saved order and open index
pages; neither custom lists nor later DOM edits enlarge selector membership.

`4chan_never_show_mobile` uses its released literal `true` value. Desktop stores
that value; either Mobile link removes it. A successful change reloads once.
Blocked reads retain mobile defaults; blocked writes report failure without
navigation. The controller ignores detached controls and suspended or replaced
documents. A later mount retires its predecessor, and cleanup preserves an
attribute replaced by another writer. The existing native layout controller
reads the same preference and uses the same mobile-mode attribute.

The archive response admits only `/static/page-chrome.v1.js` as script code.
Fetches, workers, sound and embedded players remain denied. Watcher and Settings
modules are absent. GET/HEAD success with HTML is required; write responses,
errors, plain responses and JSON-shaped routes retain no script authority. The
asset itself serves fixed GET/HEAD bytes, has no API-listener or write route,
and grants no imports, worker creation or network authority when used as code.

## Verification and corrections

The component replay exposed theme rules overriding navigation hover colors
and missing space below the fixed mobile bar. Initial reference fixtures also
selected mobile CSS by desktop theme. Public server links and the unchanged
Core switcher establish separate safety-specific mobile selection; the corrected
record includes both safety settings and the catalog loader's theme class.
Source page minimum height is zero, avoiding extra height on short mobile pages.

The first full Windows theme run passed 754 of 789 cases. Ten mobile dark-mode
cases found a real regression: the added body color overrode released mobile
post colors. A scoped dark-mode rule restores those colors; all 24 post-header
cases, covering 288 property comparisons, pass in the focused correction run.
Settings tests now wait for the binding-ready attribute on server links. The
drop-down lifecycle check requires the complete directory and separate custom
links. The diagnostic body color follows the newly qualified explicit background.
All 63 focused lifecycle, shortcut, updater, post-header and diagnostic cases pass.

The first complete rerun passes all 789 theme cases. A subsequent whole-extension
CSS check found uncovered dark navigation, title and inherited colors. Its new
application case failed before correction. The first correction passed 24 of 48
cases: base-theme dark rules masked unchanged colors in three other themes.
Complete qualified component properties restore those exceptions. All 48 dark
cases now pass on Windows; the reference record also matches without rewriting.
The source's footer-link hover keeps its normal blue because its specific
important rule outranks the ordinary dark hover rule. Photon retains its blue
title, while Yotsuba B retains its red banner/subtitle.

Visual review covers owned board, catalog, archive, media, empty-page and theme
fixtures. The first 49 accepted Windows images show the changed navigation,
title, footer and page spacing. Six separately reviewed mobile text-table images
bring the total to 55. Text table captures can move into the existing 200-pixel
public background gradient. Screenshot comparison retains zero allowed different
pixels. The final Windows run passes all 837 theme cases, including all 48 added
dark component cases.

Windows also passes three board/catalog visual cases, six archive cases, all 68
media cases and ten empty/error-state cases after the dark correction. The first
affected database-backed group passes 115 of 117 cases; two fail before setup
because the isolated worktree lacks the navigation fixture binary. Building all
public examples resolves setup, and the final group passes all 117 cases.

Linux passes all twelve PostgreSQL-backed navigation, Settings and archive cases.
The five new cases cover script-free real directories, all three bound thread
Settings links, mobile catalog destinations and mode reloads, cross-tab storage,
and read-only archive navigation under its restricted CSP. Owned threads are
created and deleted through real handlers. Archive setup and cleanup use the
owned fixture helper and keep migration credentials out of the public server.

Windows and Linux each pass 28 navigation/controller cases and nine display core
cases. Public library and asset/CSP checks pass: Windows has 90 library cases and
Linux has 94, including four Linux-specific transport cases; each has nine asset
cases. Formatting, generated assets and public all-target/all-feature Clippy
checks pass on both. The first Windows generated-asset check found the isolated
worktree lacked local dependencies; installing its pinned lockfile resolved it.
Linux passes the final 286-case group: 192 ordinary component/hover cases, 48
dark component cases, 24 mobile post-header cases and 22 Settings lifecycle cases.
Its core, library, asset, Clippy, formatting and generated-asset checks also pass
after the dark correction. All three original client replay records match without
rewriting them. Hosted qualification runs on the pull request; all required checks
must pass on its final commit before merging. This record does not claim complete
page or rewrite parity.

```text
npm run test:navigation-core
node --test tests/browser/native-display.test.mjs
npx playwright test --config playwright.config.js tests/browser/public-page-chrome.spec.js tests/browser/native-navigation.spec.js tests/browser/native-display.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/public-page-chrome.spec.js tests/themes/public-page-dark.spec.js
npm run test:themes
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:states
cargo test -p board-public --lib --test ui_assets --locked
```
