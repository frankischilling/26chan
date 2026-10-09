# Source parsing lifecycle events

The pinned source is `26chan-reference/js/extension.js`. `Parser.parseThread`
(line 1055) computes a normalized start from the offset, then adds a truthy
limit argument to that start. Its `4chanParsingDone` payload (line 1098) reports
that start and the exclusive end, not a count. A zero or omitted limit means the
whole remaining thread. Negative offsets count from the current post length.
The helper preserves this arithmetic rather than clamping it.

The native client now emits an ordinary, non-bubbling, non-cancelable `Event`
on `document`, with an assigned `detail` containing only `threadId`, `offset`
and `limit`. This follows `UA.dispatchEvent` (lines 8768–8775).

- Initial thread and board bootstrap publishes each canonical thread once,
  after receipt consumption, read acknowledgement, rendering and worker-backed
  filters settle. Updater, expansion and depager actions stay gated until these
  initial events finish, including during callbacks from event listeners. Their
  controls render normally before completion; only the data-loading actions wait.
- Updater append publishes its new suffix before `4chanThreadUpdated`.
- Expansion publishes offset 1 and an exclusive end of 1 plus the number of
  inserted replies, before the local `4chanThreadExpanded` event. This matches
  the source's `(tid, 1, i - 1)` call at line 6058.
- Depaging publishes one full range per newly committed thread, before the
  local `4chanPageDepaged` event. Already-present threads do not publish again.

Captured ranges check section and post identity, parentage, connection, order
and count after asynchronous application. Failed or cancelled applications,
replacements, detached nodes, disabled controllers and rolled-back additions
cannot publish completion. A captured range can publish only once. Collapse,
re-expansion of an already loaded range, ordinary re-renders, quote previews,
inline copies and BFcache restoration after completed initialization do not
create parsing events. Interrupted initialization retries on restoration with
a fresh cancellation signal; already-published initial ranges are not repeated.
Auto-update countdowns begin only after readiness. A queued Quick Reply refresh
is retained while initialization is pending.

Post selection uses direct canonical post containers. Nested quote copies are
outside the range. Initial, updater and expansion IDs remain strings, matching their source DOM
attribute provenance. Depager IDs are numbers when safely representable,
matching the source's JSON post numbers. Depager IDs above JavaScript's safe
integer range remain exact decimal strings. This narrow representation boundary
avoids rounding two different server IDs to the same value. These public events are notifications, not trusted input
or permission to insert markup. Existing snapshot validation remains in place.

## Main initialization

Board and thread pages emit `4chanMainInit` once after watcher context and
startup preferences are established, before math or post features mount. The
event has no detail payload. It still fires when `disableAll` is set, matching
source `Main.init`; enabled-board math also remains independent of that option.
Upload pages do not emit it.

Math imports no longer mount a controller. Watcher owns the explicit factory
call and shares its projection with the other post features. If the import
finishes while a document is suspended, mounting waits for persisted restoration.
A second suspension keeps it waiting; a terminal departure cancels it. The
existing parsing transaction then consumes receipts and settles filters before
publishing `4chanParsingDone`. Neither event repeats after completed BFcache
restoration or a settings change. A failed math import leaves literal tags and
ordinary parsing available.

The pinned source contract is recorded in
`tests/fixtures/native-main-init-source.json`. Regenerate or check it with
`scripts/extract-main-init-reference.py` against the pinned reference checkout.
The fixture executes the original `Main.init` and dispatcher and records the
parser and independent core-math boundaries. It does not claim that asynchronous
math rendering finishes before `4chanParsingDone`.

Catalog pages have their own initialization boundary. Source `catalog.js`
applies sort, image-size and teaser preferences before emitting `4chanMainInit`.
The caller in `catalog.php` then invokes `loadCatalog`, which loads thread state,
restores search and builds the catalog. Native catalog controls follow that
order while retaining the existing server-rendered cards.

Catalog preferences and watcher settings meet through a private controller keyed
to the current `watcher-context` node. Settings/navigation setup and initial theme
application precede the event. The controls then reflect validated saved values
or explicit URL overrides. Only after notification does native catalog loading
restore pin/hidden state and search, rebuild cards, and start filters. Defaults
remain bump order, small images and teasers on. Blocked or malformed storage keeps
the existing finite defaults; initialization does not save catalog display
preferences. The event still fires with `disableAll`.

The controller waits through persisted suspension and checks the original roots
again after event listeners run. Replaced roots and terminal departure cancel the
pending work. Completed initialization never repeats on restoration, settings
changes or filter completion. Public events cannot release this private gate.
The GET Apply button, search submission and Reset link remain usable until catalog
loading starts. Changes to display controls during a delayed startup or inside a
MainInit listener are queued until per-thread controls exist; the latest choice
is then applied and saved. Typed search text is also retained, including an explicit
empty query that clears the previous session search. A queued spoiler choice
triggers thumbnail rendering even when no other preference changed.
An optional settings-controller failure leaves basic catalog controls usable.
The catalog owner observes the watcher's existing module load through the module
map. A rejected optional load continues catalog preparation without a Settings
controller; it does not retry transport or rely on a public readiness event.
The pinned `catalog.php` caller runs `fourcat.init()` and `loadCatalog()`
independently, and `catalog.js` dispatches MainInit from its own initializer.
Standalone synthetic identifiers that the watcher cannot accept retain their
independent controls instead of waiting for an owner that cannot start.
If an essential catalog module cannot load, server-rendered GET controls remain
available without a MainInit notification. Invalid catalog snapshots retain their
existing fallback and do not announce an applied-preferences boundary.

This work does not add legacy `Main` or `Config` global objects.

## Verification

Verified locally:

- `npm run check:native-thread-controls`, including the pinned filter check
- `node --test tests/browser/native-source-events.test.mjs`: nine tests cover
  range arithmetic, exact IDs, duplicate emission, ownership changes,
  asynchronous settlement, deferred receipt/filter ordering, blocked early
  updates, safe numeric depager IDs, rejection, cancellation and disablement
- `node --test tests/browser/native-source-updater-readiness.test.mjs`: two
  actual-controller tests use fake timers to cover slow startup, auto-update
  scheduling, BFcache resumption and a queued Quick Reply refresh
- Pure expansion planning tests, including adjacent unsafe-integer IDs
- Syntax checks for the new and updated browser suites

Browser execution is pending on the qualified hosted runner; native browser execution
is unavailable in the local executor. `source-parsing-events.spec.js` registers a listener before the
real application starts, holds receipt Web Locks to attempt an early update and exercise restoration
during pending initialization,
and checks thread/board bootstrap, an updater append,
event order and no settings/BFcache replay. Existing isolated updater,
expansion and depager browser suites now check dynamic ranges and event order;
updater tests also cover callback rejection, cancellation and replaced nodes.
These browser assertions are added but are not claimed as passing here.

## MainInit qualification

Local checks cover the pinned source event, inert math imports, the actual
watcher boundary on board, thread, disabled, catalog and upload contexts, and
import interruption before and after BFcache restoration. Existing receipt,
filter and dynamic-action readiness tests remain in place.

The browser suites now inspect MainInit before real math and parser startup,
exercise delayed and failed math imports, check interruption from the event
listener itself, and verify QR still receives its TeX controller after resumption.
The parsing suite also checks MainInit while receipt locks hold ParsingDone back.
These assertions still need execution on the qualified hosted runner. The local
Playwright Chromium executable is missing; no browser pass is claimed.

## Catalog qualification

The source fixture pins `js/catalog.js` and the `catalog.php` caller in addition
to the board/thread sources. Source-executing tests run the catalog initializer,
defaults, setters and DOMContentLoaded caller. They inspect the controls at
MainInit and verify that initialization has not saved display preferences or
built threads.

The following checks were rerun against the reconstructed checkout on October 8,
2026. `npm run test:source-parsing-core` passed all 47 tests, including 19 catalog
cases. The catalog theme and position tests passed all 12 cases. The pinned
extractor check, changed-JavaScript syntax checks and `npm run check:generated`
also passed. Generated checks required a local copy of the locked dependencies;
the source allowlist remained unchanged.

The catalog tests cover delayed settings, optional-loader failure, suspension
during theme setup or the MainInit listener, a second suspension, root replacement
and terminal departure. Tests also execute the production preference handlers
and initializers to check early display choices, GET submission and Reset,
an isolated queued spoiler change, and an explicitly cleared pending search.

`catalog-main-init.spec.js` covers default, saved, URL-overridden, malformed and
unavailable preferences, disabled extensions, empty and text catalogs, delayed
watcher startup, listener interruption, and an essential module failure. It also
checks early display/search interaction, synchronous MainInit control changes,
and a failed optional watcher module with working native display, Enter search
and Reset. A missing essential catalog module retains the GET fallback.
Separate cases cover an isolated MainInit spoiler choice and clearing a pending
search. These browser cases have only been syntax-checked in this checkout.
Playwright's configured Chromium executable is absent, and the production suite
requires its Cargo server and database setup. Browser execution remains pending;
no previous workspace result is treated as a pass for these reconstructed files.

## Windows catalog repair, October 8, 2026

The current Windows checkout has the pinned Chromium runtime. A targeted run
reproduced four hosted failures: blocked watcher startup in two catalogs,
text-catalog menus and preview-page pinning. The mobile textarea shortcut case
passed. After the repair, all five scenarios passed against the actual visual
fixture server, including the existing menu screenshot. No baseline changed.

All 48 source-parsing/MainInit Node tests, the source extractor, generated-asset
checks, changed JavaScript syntax and diff checks passed. The broader page
suite requires the real development database and browser fixture; an initial
visual-server invocation was stopped because it lacked those page routes and
is not counted as validation. The owned PostgreSQL 16 roles, migrations and
seed were then installed for database-backed page qualification.

After building the `browser-tests` fixture, all 18 cases in
`npx playwright test --config playwright.config.js tests/browser/catalog-main-init.spec.js`
passed on Windows against that migrated database and the actual public server.
The failed-watcher test starts without a restored session query, checks native
search, restores the initial thread count and requires the exact default display
URL after Reset. The essential-module test still requires the GET fallback and
no MainInit event. Fresh hosted qualification remains required before merge.

The standalone catalog lock fixture now serves `thread-watcher-core.v1.js`,
which the catalog bootstrap imports to validate board identifiers. Both Linux
and Windows CI stopped at the fixture's module allowlist assertion before the
lock scenarios ran. After adding the actual module to that fixture,
`npm run test:catalog-preference-locks-core` passed all 13 cases on Windows,
including Chromium lock contention, Reset, stale writes, cross-tab restoration,
BFcache suspension, root detachment and storage denial. The allowlist and race
assertions remain in place. The corrected head still requires hosted CI.
