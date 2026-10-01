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

The [viewport record](public-viewport-reference.json) separately covers the
document background and one opaque pixel below a short owned body. Its 336 cases
include six themes, board/catalog pages, both safety values, four responsive
widths and two densities, plus narrow-width mobile opt-out and index dark-class
states. Whole pinned Core and extension releases run their navigation and CSS
entry points; only the two pinned public gradient PNGs can load. The catalog
theme class follows the pinned catalog client. Windows and Linux source replays
match the saved record without rewriting it.

The application keeps the document background transparent on public board pages
so the body's background paints the viewport. The initial 24-pair probe finds
twenty bottom-pixel mismatches, including mobile dark pages that expose a light
background below the footer. The frozen regression then fails with expected
`[29,31,33,255]` and actual `[255,255,238,255]`. After the correction, all 336
application cases match the recorded root properties and pixel on Windows and
Linux. This
qualifies viewport paint below the body; it does not compare complete page
pixels, body height, content placement, original startup or catalog dark mode.

```text
node scripts/verify-public-viewport-reference.mjs <reference-directory>
npx playwright test --config playwright.themes.config.js tests/themes/public-viewport.spec.js
```

Directory rendering is capped at 100 boards and retains the current board when
it lies beyond that set. Template escaping applies to titles and descriptions.
HTML board, catalog, thread and archive readers load that directory within the
same repeatable-read transaction as their content and settings. Rendering starts
after commit. JSON and updater reads retain their existing bounded queries and
do not load the HTML directory.
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
rewriting them.

The first hosted Linux run fails the existing board snapshot concurrency test:
the HTML handlers read navigation after committing their content snapshot, so
all ten index/catalog modes combine old content with a newly committed board
title. The correction moves the bounded directory read into each HTML snapshot.
The expanded lock-witness test checks 32 reads across index/catalog, active and
archived thread HTML, archive HTML and both JSON listeners, including deletion.
Another owned board changes its title and worksafe state in the same commit;
HTML controls prove that this second board appears before and after the change.
Every concurrent response must equal a complete before or after response,
including its status. Fixture creation uses one transaction and cleanup removes
both owned boards after an assertion failure.

Windows passes eight real-database cases across the expanded board test, thread
snapshots, archive routes, updater responses, store archive rollover and store
concurrency. Linux passes the same eight plus the tail/policy/privilege case with
its separate staff role. Initial coverage setup fails on incomplete board bounds,
an archived sticky thread and an implicit settled sticky state; these fixture
errors are corrected. The first tail run lacks staff credentials; sourcing the
existing owned staff configuration resolves it on Linux. Windows does not run
that staff-dependent case locally. These setup failures do not replace or weaken
the original concurrency assertion.

After the snapshot correction, Windows passes the twelve real-handler browser
cases again. Public library/asset checks, formatting and generated-asset checks
pass on both platforms. Windows passes all-target/all-feature Clippy for the
public and store packages; Linux passes the full workspace equivalent. The
broader Windows workspace attempt stops while building the staff application's
vendored OpenSSL because Perl is unavailable. That attempt is not counted as a
passing workspace check.

The second hosted Linux run passes the snapshot tests, then fails the text-only
test because it requires the entire body class to equal `text_only`. The public
navigation adds other body classes. The correction reads class tokens and still
checks text-only policy both on and off, upload-button visibility, required
subjects and locked admission. The broader Linux public all-feature Rust suite
passes after that correction, including its real upload-browser case. The first
viewport regression setup inherits script-free visual configuration; explicitly
enabling JavaScript resolves setup before the measured root/pixel failure is
recorded.

The first complete Windows run after the viewport correction passes 1,170 of
1,173 cases; the other three still expect an opaque root background. Preference
and stylesheet-failure checks now require a transparent root and check the
body's displayed color. All seven focused preference/diagnostic cases pass.
The Tomorrow mobile expected/actual/diff images are inspected individually:
only exposed background bands above content and below the footer change from
the old dark root to the recorded mobile body blue. That one frozen image is
accepted; no other baseline is refreshed for this correction. The latest review
hash for every one of the 55 changed Windows baselines is verified.

The first affected media group passes 67 of 68 cases; the hover geometry case
still reads its expected visible color from the now-transparent root. It now
reads the body color while retaining the opaque-background, exact preview-color
and geometry assertions. All 68 media cases pass after the correction. Windows
also reruns all three board/catalog, six archive and ten empty/error-state cases
successfully. Screenshot tolerance and retries remain zero. The next hosted
Windows run must pass the complete 1,173-case theme group on the final commit.

The third hosted run passes the workspace Rust tests, then fails the catalog
browser CSP check because its exact script list omits the new fixed navigation
module. The list now includes only that module. A local Linux rerun passes the
policy and denied-script controls, then finds the older one-script element
count. The assertion now requires the exact two module paths and types, with no
inline script element. The broader behavior suite must pass after these changes.

The broader local Linux run passes the first 58 browser cases and the subsequent
position, shortcut, menu, thread/reply-hiding and watcher groups. Its first
archive-lifecycle attempt cannot find an older helper's conventional Cargo
target path in the shared-target guest. The local launcher supplies that path
and builds the remaining example helpers. The remaining group then finds two
text-only browser cases with the same whole-class assumption as the earlier
Rust test. They now check the `text_only` token while retaining required-subject,
subject-only OP, subjectless reply, absent upload and API policy assertions.
The remaining group passes 79 of 81 before that assertion correction. All three
focused subject-policy cases pass afterward. Windows passes all eight catalog
preference/CSP and subject-policy cases after both browser assertion corrections.

That hosted Windows run passes 1,172 of 1,173 theme cases. The remaining case
fails at navigation with `net::ERR_NO_BUFFER_SPACE`, before comparing the owned
catalog state. The saved failure screenshot is a blank page. Before themes,
the host reports 718 TIME_WAIT sockets and about 13.3 GiB of free memory; after
the group it reports 796 and about 13.4 GiB. These samples do not establish the
resources at the failing navigation or its cause. The catalog-control fixture
now uses the existing bounded visual diagnostics, including failure-time host
aggregates when CI enables them. No retry, pixel tolerance, baseline or test
selection is changed. This failure remains recorded; a fresh complete hosted
run is required.

The fourth hosted Windows run on `26dbd376f7ffb19eb9533b02bcbb1f6ff27c956e`
passes the complete 1,173-case theme group and the other visual groups. This
does not identify the cause of the earlier navigation failure. The final
text-only browser assertion change still requires all five hosted checks on
its own commit before merging.

Hosted qualification runs on the pull request; all required checks
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
