# Thread watcher

The watcher remains under implementation in issue #88 and draft PR #89. It is
not yet a complete reproduction of either public client.
The extension connects native filters to manual automatic-watch refreshes,
persists unwatch suppression, and provides a filter editor with page hiding and
highlighting. The compatibility and qualification work below remains required.

## Native filter editor and page effects

Settings now exposes Filters & Post Hiding, its Edit link, filtering enablement
and hidden-thread stubs. The editor preserves ordered `4chan-filters` rows with
On, Pattern, Boards, Type, Color, Auto, Hide and Delete controls. Add, move-up,
delete, native palette selection, validated custom colors and nested help work
with keyboard focus restoration. Legacy type 3 becomes ID type 4 only when the
editor loads it; saving an empty list removes the storage key.

Save validates active patterns in a disposable worker, then compares the
original serialized rules with current storage under the shared Web Lock.
Competing edits retain the open draft rather than overwriting the other tab.
Closing the editor aborts validation and prevents a queued save from committing.
Unavailable writes retain editable rules in the current tab without changing
the persisted list. The editor stays open and states that the save applies only
to this tab when storage or locking is unavailable. Further edits compare against
that same-tab draft. A separate page warning remains after the editor closes;
it disappears on navigation, which also discards those volatile changes.

Page matching is distinct from catalog discovery: blank board scope is global,
subject filters require a present subject and do not run inside thread pages,
missing comments and filenames become empty strings, and tracked own posts are
exempt. Thread-page OPs are not filtered. The first matching active rule consumes
the post even when it has no highlight color. On board indexes, OP hiding affects
the whole thread; View navigates to the thread. Reply View reveals content in
place. Hide Stubs retains a stub for sticky threads.

Patterns and HTML preparation remain inside bounded workers. Page batches use
the existing field, post and request limits, with at most 20,001 DOM posts and
16 MiB of serialized page fields per refresh. Failed matching leaves posts shown.
The renderer uses text nodes and parsed color values, not arbitrary CSS or HTML
from saved preferences. Colors cannot contain declarations, custom-property
substitutions or inherited values. Filtering is not applied to catalog cards.

`npm run test:page-filters` passes fourteen owned Chromium/PostgreSQL cases covering
editor persistence, order and palette, effects, first-match precedence, own-post
exemption, cross-tab conflict, queued-save cancellation, failed writes and hostile
input. One case exercises all six themes at 1280px and 390px with nested-dialog
focus checks. The sticky exception is tested through an explicit DOM fixture,
not a staff mutation. These tests establish behavior, not screenshot parity.

### Filtering selected text from a post menu

With filtering enabled, board/thread post menus expose the pinned extension's
`filter-sel` action, "Filter selected text". Opening the menu captures the current
selection so moving keyboard focus into the menu does not discard it. The editor
appends an unsaved active row with trimmed pattern text, blank/global board scope,
and Auto/Hide off. The user still chooses effects and saves through the existing
worker validation and conflict-aware storage path. Cancel does not persist it.

The type follows the selection anchor's parent: Name, Tripcode, Subject, poster
ID, Filename or otherwise Comment, as in extension v1191's `Filter.addSelection`.
The release filename anchor is recognized alongside the native `fileText` class.
Selected text remains an input value, not HTML. Text beyond the 1,024-character
pattern bound is rejected visibly instead of silently truncated. Disabling
filtering in another tab removes the action from an already-open menu.

Browser cases cover persisted Name filtering, other type selections, cancellation,
focus return, literal hostile text, mobile operation, oversized text and cross-tab
disabling. Tripcode/ID/filename selection tests use explicitly inserted UI fixtures;
they do not claim backend posting support for those fields. Other native post-menu
actions and the optional keyboard shortcut remain outside this implementation.

## Owned archive and expiry lifecycle

`tests/browser/watcher-archive.spec.js` uses the existing synthetic archive-board
helper and real public posting handlers. A second thread rolls over its owned
one-thread board. The watcher retains unread state, applies the archive class,
and acknowledges replies when the user opens the archived read-only thread.

The fixture-only `expire` command ages both archive timestamps in their required
order, within a transaction. It requires the migration identity, the synthetic
board marker and exactly one archived fixture thread. Those credentials remain
in the bounded helper process, not the public server or browser. The test then
checks actual 404 responses, an empty archive listing, the watcher's dead state,
and removal on the next refresh without another request for the dead thread.
Cleanup is limited to the helper-created board.

`npm run test:watcher-lifecycle` passes this case and the four existing deletion,
failure and delayed-response cases. The new case initially used a boolean where
native storage writes `1`; its first expiry fixture also allowed expiry to
precede archive creation. Both fixture assertions/timestamps were corrected
without changing application constraints, retention policy or watcher behavior.
This is controlled fixture aging, not a deployed retention or clock-skew test.

## Unavailable receipt access

The posting browser suite uses the pinned Chromium protocol's
`Emulation.setDocumentCookieDisabled` control, with a working cookie read before
disabling it. Real posting still succeeds. Chromium retains the server's network
receipt, but the extension creates neither an automatic watch nor an own-post
hint while the document API is unavailable. Restoring access and reloading
consumes and clears that receipt through the normal client path.

This checks unavailable `document.cookie`, not every browser cookie policy or
network-level rejection of Set-Cookie. Receipts remain optional public-ID hints,
not proof of ownership or authentication. Further production receipt integration
still requires its own evidence.

## Approved-upload tracking

The public upload browser harness runs with and without JavaScript for both
text-plus-image and image-only posts. JavaScript cases enable the actual watcher
settings, check the approved form's `track` and `awt` fields, and verify successful
post receipts, automatic watching, own-OP tracking and receipt cleanup. A normal
reply to that uploaded thread uses the board-return option, emits its own tracking
receipt without `awt`, and appears in tracked own replies after navigation.

All four variants retain image, API metadata, validator, spoiler and file-only
deletion assertions. The JavaScript catalog assertion checks the reply label,
numeric count and menu control separately; its menu marker is not reply data.
Deletion targets the OP explicitly when a tracked reply is also present. Existing
no-JavaScript screenshots keep their names; optional JavaScript captures use a
distinct suffix rather than overwriting those captures.

`cargo test -p board-public --test upload_browser --all-features --locked --
--test-threads=1` passes one integration test exercising all four variants. Its
supervisor supplies validated synthetic pixels through the real publication path;
this local test does not execute a decoder or establish a VM containment boundary.
The owned Linux fixture also includes a JavaScript PNG case through its actual
intake/Firecracker/promotion/reader pipeline. That added case requires a completed
hosted run; compiling its Python harness is not equivalent evidence. Production
media remains disabled and no runtime authority or origin policy changes here.

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

Other native settings and post-menu actions remain unfinished. The filter editor
still needs reviewed native visual comparisons. Additional storage-race coverage, reviewed
full watcher screenshots and passing exact-head CI are required before merge.
Production media and deployment qualification remain separate requirements.

## Settings integration

The catalog exposes its watcher checkbox through an Options dialog with the
native `theme`, `theme-tw`, `theme-save` and `theme-close` identifiers. Saving
applies in place; enabling the watcher clears `disableAll`, as in catalog v1025.
The board/thread dialog uses the extension's Monitoring labels for watching,
automatic watching after posting and fixed positioning, plus the global disable
override. Persisted board/thread saves navigate without the fragment, as in the
extension. Filters & Post Hiding exposes filtering, the editor and hidden-thread
stub preferences. Other native settings categories are not implemented by this slice.

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
catalog/thread pages at desktop/mobile widths, checks decoded image dimensions,
watch toggles, the busy/error refresh transition, close/reopen, catalog placement
and keyboard access. `tests/browser/behavior.spec.js` loads all pinned images
under the real server's CSP and checks denied-origin and unlisted-path violations;
the synthetic visual-fixture pages do not send CSP headers. `apps/public/tests/ui_assets.rs`
checks fixed bytes against both image manifests, GET/HEAD behavior, response
headers, denied writes and missing paths. Test results are recorded per commit
in the PR; adding this coverage is not itself a passing result.

This is not a full watcher parity claim. The native filter editor,
additional settings and position edge cases,
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

## Board and thread post-menu watching

Extension v1191's `Parser.parsePost` appends the desktop post-menu control to
post metadata and prepends its mobile ellipsis counterpart. `PostMenu.open`
places the menu below the control, clamps its right edge to the viewport, and
offers Add to/Remove from watch list for OPs when watching is enabled. Board
pages use this menu instead of the temporary inline watcher leaf. Thread-page
OP menus share the same state as their four navigation controls. Replies do
not offer a thread-watch action. Catalog controls are unchanged.

The menu and trigger rules come from the pinned extension and public desktop
v716 stylesheets; the mobile rules are in the v716 stylesheet recorded in
`public-watcher-navigation-reference.json`. Desktop menu font sizes, borders
and colors follow each of the six themes. Mobile controls use the observed
480px breakpoint, rotated ellipsis and 16px/2.5em menu typography.

Menus use text nodes and semantic buttons instead of the reference's HTML-string
construction and click-only list items. Arrow keys, Home/End, Escape and Tab
support keyboard navigation; focus remains visible. Outside activation,
viewport changes, page exit and global disabling close an open menu. Open watch
actions update after cross-tab changes. Watch changes use the existing bounded
storage and lock path, including its same-tab fallback.

Report post and mobile Delete post open and focus the existing per-post forms.
Selecting an action never submits a report or bypasses the deletion-password
gate. The no-JavaScript forms remain intact. This reuses the rewrite's existing
safe form workflow; the original report popup is not reproduced here. No new
script/CSP permission, network request, database grant or migration is added.

`tests/browser/thread-watcher.spec.js` covers actual persisted watch, report
and deletion operations, cross-tab menu updates, optional-storage failure,
keyboard dismissal and global disabling. `tests/themes/post-menu.spec.js`
covers all six themes at desktop/mobile widths. Results are recorded per commit
in the draft PR; adding tests is not itself evidence that they passed.

`npm run test:behavior` runs the general browser cases, watcher cases and
post-tracking cases in separate owned server invocations. This keeps each
fixture group within the existing per-peer write budget without raising limits
or skipping tests. Actual rate-limit enforcement remains covered by the Rust
HTTP-limit tests.

Native hiding/filter actions, media actions and the remaining settings are not
implemented by this menu slice and are not exposed as placeholder controls.
Full reference screenshots, filter-driven watching/blacklisting and the other
gaps above remain required before full parity or merge can be claimed.

## Filter matching worker under development

`native-filter.v1.js` implements the pinned filter matching contract for prepared
catalog fields. It is served as a fixed release module but is not yet connected
to the watcher. Catalog fetch,
safe HTML-to-text field preparation, blacklist transactions, the filter editor
and complete browser integration remain required before automatic watching
is usable. No inert filter controls or permissive worker routes are exposed.

In extension v1191, active Auto filters with explicit boards select which
catalogs to fetch. `Filter.match` then considers all active filters explicitly
scoped to those boards, including filters without Auto. Blank scopes do not
match in this catalog path, unlike the separate on-page `Filter.exec` path.
Board tokens retain case and encounter order; the native token loop stops if
the first token is empty. The worker does not silently normalize those rules.

Tripcode, name and poster-ID patterns are exact strings. Comment, subject and
filename patterns use native JavaScript regular expressions, quoted patterns,
or word-boundary AND terms with non-whitespace wildcards. The native escaping
list omits `|`, so alternation survives even inside quoted patterns. Unquoted
AND matching is line-sensitive. Empty comments are skipped; missing subject
and filename fields are coerced to the string `undefined` by native RegExp.test.
These observations come from `Filter.load`, `Filter.match` and
`ThreadWatcher.refreshWithAutoWatch` in the already pinned extension. Its code
was inspected as text, not executed.

The caller validates and copies bounded data without compiling user patterns.
Each match uses a fresh worker, terminated on completion, cancellation, error
or a one-second deadline. No unavailable-worker fallback runs a pattern on the
main thread. Results contain only input post IDs and filter indices; invented,
duplicate, reordered or out-of-scope selections are rejected. IDs remain exact
strings across the full positive i64 range. The worker receives prepared plain
comment text, not raw HTML; preparation still needs separate compatibility tests.

Security bounds are 64 filters, 1,024 code units per pattern, 32 selected boards,
512 posts per job, 16,384 code units per field, a 2 MiB-code-unit request and a
64 KiB-code-unit response. Malformed settings or a compilation error reject the
whole operation instead of applying a partially compiled filter list. Legacy
type 3 migration belongs in the pending editor; the matcher accepts the six
types emitted by the current pinned editor. These are bounded-state and
failure-handling exceptions, not claims that the native client has these limits.

`native-filter.test.mjs` checks matching and protocol bounds through owned Node
worker threads, including termination of a bounded pathological-regex fixture
and caller responsiveness. `native-filter-worker.spec.js` exercises the actual
browser module worker and its deadlines. It also uses the real script response
headers with an owned probe body to test denied network access, imports and
nested workers, with healthy browser controls. The parent permits only the
exact module URL in worker-src; noninteractive responses permit no workers.
Script-resource response policies deny network and imports in worker contexts.
The Rust asset test checks fixed bytes, HEAD, denied writes and response headers.
These checks do not qualify media-worker containment or the unfinished automatic
watching workflow. Passing results are recorded per commit in the draft PR.

### Filter worker checkpoint validation

The owned Windows/Chromium/PostgreSQL validation passed with the worker served
at its fixed release URL. The matcher is not yet connected to automatic watching;
catalog preparation, transport, blacklist transactions and the native editor
remain required.

- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p board-public --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test -p board-public --all-features --all-targets --locked -- --test-threads=1`: 74 test executions passed, including the visual-fixture example test.
- `npm run test:behavior`: 45 unit tests and 60 browser tests passed, split into 46 general, 11 watcher and 3 post-tracking browser cases.
- The two targeted `native-filter-worker.spec.js` browser tests also passed separately.
- Board/catalog, archive, media and full theme suites: 3, 6, 14 and 88 tests passed respectively. No baseline changed.

The original worker-denial probes terminated workers immediately after
construction and therefore mislabeled asynchronous startup failures. Both now
await a healthy message or startup error, terminate on every completion path,
and fail distinctly on a two-second timeout. Exact CSP-violation URL assertions
remain; denied module imports report the effective `script-src-elem` directive.
The healthy unrestricted worker control still runs before the denial probes.

These results qualify this local checkpoint, not the complete watcher or 1:1
frontend. Exact-head CI remains a separate gate, and production media stays off.

### Catalog parser and prepared-comment contract

`native-catalog.v1.js` parses catalog JSON without converting numeric post IDs
through JavaScript Number. It retains exact positive i64 ID strings, source page
and thread order, and only the raw fields used by filters. It rejects duplicate
JSON keys, duplicate pages or threads, malformed field types, and inputs exceeding
the source, nesting, value-count, container, page, post or string budgets. Limits
on strings are UTF-16 code units; transport byte limits remain separate. Oversized
or malformed catalogs fail as a whole rather than applying partial matches.

The parser does not decode HTML, fetch catalogs, or add watches, and it is not yet
served as a release asset or connected to the watcher. Raw `com` presence and text
are retained for the next conversion stage. That stage must omit prepared
`comment` when raw `com` is absent or empty. A nonempty raw comment whose HTML
parses to empty text instead produces a present `comment: ""`, which the worker
must test normally. This distinguishes `<span></span>` from an absent comment
for `/^$/` filters. Node and actual browser-worker regressions cover the prepared
field distinction; HTML decoding itself still needs reference and security tests.

Parser checkpoint validation on the owned Windows/Chromium/PostgreSQL setup:

- `npm run test:behavior`: 54 unit tests and all 60 browser tests passed, retaining the separate general, watcher and posting server invocations.
- The two actual worker browser tests also passed separately; the normal worker case now checks the empty-present versus absent comment distinction and termination of all five worker jobs.
- `cargo fmt --all -- --check` and all-target, all-feature public Clippy passed.
- `cargo test -p board-public --test ui_assets --all-features --locked`: all three release-asset tests passed with the changed worker bytes.

The prior 74-test Rust and 111-test visual/theme results apply to the earlier
worker checkpoint, not this parser change. No screenshot baseline was changed.

### Bundled worker HTML conversion

The worker now accepts either bounded raw `com` or prepared `comment`, rejecting
ambiguous requests that contain both. Raw HTML is converted only when a comment
filter is reached. The worker preserves the pinned literal `<br>` substitution
and unusual character-class replacement, then uses parse5's HTML div-fragment
context with `scriptingEnabled: true`. That option selects native `noscript`
text parsing; it does not execute JavaScript. The tree consists of ordinary data
objects, never browser DOM nodes, so markup does not create resource requests,
event handlers, frames or custom elements. Text traversal excludes a template's
separate content fragment and preserves parser text order and entity decoding.

Raw HTML is bounded to 65,536 UTF-16 code units per field; decoded text keeps the
16,384-unit field bound. Element creation and tree traversal each have a 65,536
node budget. The existing request and worker deadline limits remain. A conversion
budget failure rejects the whole matching result, not a partial list. Filters
that never inspect comments do not invoke the HTML parser.

The worker source is now under `apps/public/client/`. `npm run build:native-filter`
bundles pinned parse5 8.0.1 and esbuild 0.28.2 into the existing fixed release
`native-filter.v1.js` URL. The lockfile pins transitive dependencies. The build
retains dependency license notices, requires one import-free browser ESM output,
limits it to 256 KiB, and checks allowed source paths and public exports.
`npm run check:native-filter` rebuilds in memory and compares exact artifact bytes;
it runs before the watcher unit tests in the behavior command. No API format,
script URL, worker URL or CSP permission was added for this conversion.

Node tests cover native text cases, including `noscript`, table text order,
templates, entities and newline normalization. A real browser-worker test covers
raw comments, absent versus empty text, budget failures, lazy conversion and an
owned healthy network control alongside markup that must not load resources.
Catalog transport, blacklist transactions, the native filter editor and complete
watcher integration still remain; this is not usable automatic watching yet.

### Owned catalog transport

The public-only GET/HEAD `/_watch/{board}/catalog.json` alias delegates to the
existing catalog snapshot function, retaining its JSON and validators without
new database grants. The API listener does not expose the private alias. The
transport and lossless parser are bundled into the existing fixed filter module;
no script or CSP URL is added. The parser source now lives under
`apps/public/client/native-catalog.js`.

`NativeCatalogTransport` uses fixed-origin, credential-free GET requests with
redirect rejection and exact response URL/MIME checks. It shares the watcher
budgets: two concurrent requests, 200 ms launch spacing, a 60-second refresh
interval, 10-second request and 60-second cycle deadlines, 4 MiB per response
and 16 MiB per cycle. Streaming bytes and fatal UTF-8 decoding are checked before
catalog parsing. Only smaller test budgets may be supplied; the refresh interval
cannot be shortened. Empty catalogs remain distinct from HTTP or parsing failure.

Every requested board receives a terminal result. Cancellation, deadline or
aggregate-budget exhaustion settles queued and in-flight work even when an
injected transport ignores abort. Late responses are discarded and their bodies
cancelled. Completed per-board data is retained in partial-cycle results, but the
transport never changes watched threads or blacklists; the future transaction
layer must only act on a current, applicable cycle and preserve failed-board
blacklist state. Native board-token case is retained without accepting path or
origin syntax from settings.

Owned HTTP tests cover limits, ordering, redirects, cancellation, late responses,
concurrency, staggering and cooldown. Real browser tests compare the alias to the
actual catalog API, exercise HEAD/304 and denied write methods, and verify cookie
omission against a healthy credentialed control. They run with the separate
watcher server invocation so existing write-rate budgets remain unchanged.
Catalog matching transactions, blacklist persistence, native editor and watcher
integration remain unfinished.

Transport checkpoint validation on the owned Windows/Chromium/PostgreSQL setup:

- `npm run test:behavior`: 72 unit tests and 63 browser tests passed, across the
  47 general, 13 watcher/transport and 3 posting cases.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p board-public --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test -p board-public --all-features --all-targets --locked -- --test-threads=1`:
  all 74 test executions passed, including database-backed cases and the shared
  visual-fixture example test.

The first browser attempt used an inactive development database port and failed
at startup. The rerun used the existing disposable native PostgreSQL instance.
The first alias method assertion expected 405 without an Origin header, but
the public cross-origin protection correctly returned 403. The test now checks
both absent-origin rejection and same-origin method rejection; neither
application protection was relaxed. No screenshot baseline changed. Hosted
checks must qualify the committed head separately before merge.

### Persisted lifecycle and delayed responses

`npm run test:watcher-lifecycle` passed all four tests on the owned
Windows/Chromium/PostgreSQL setup. The behavior command includes this suite with
its own server invocation, preserving the existing request-rate budgets.

The tests create synthetic threads and replies through ordinary public posting
and delete only their own posts through the password gate. They verify that
reply deletion retains an already observed unread count, OP deletion produces
the native dead row, and the next refresh removes that row without another
thread request. A healthy owned API response precedes injected HTTP, MIME, JSON
and encoding failures; each failure preserves the exact stored watch tuple,
and a later real response successfully updates it.

Two actual browser tabs exercise unwatch and read acknowledgement during a
held response. The held bytes come from the healthy owned thread API. A bounded
test wrapper ignores the abort signal so the response really arrives after the
storage event. Neither delayed response resurrects an unwatched thread nor
overwrites the newer read position. The test waits for actual late delivery
before checking both tabs' saved state.

The initial deletion test checked the idle flag before refresh acquired its
Web Lock. Its helper now waits for the terminal notice as well as the idle
flag; the corrected full four-test suite passed. No application code or limits
changed. This extends the earlier transport checkpoint evidence; it does not
establish archive/expiry behavior, full storage-race coverage, filter-driven
watching or complete visual parity.

## Filter-driven watching and blacklist persistence

The extension's manual Refresh control now reads `4chan-filters` when the
native `filter` preference is enabled. Active Auto rows select the boards;
all active filters explicitly scoped to those boards can match, retaining
first-match order. Catalog-page refresh and automatic page initialization keep
their distinct reference behavior. A manual extension refresh can discover
threads when the watch list is empty. The editor is not implemented yet; the
integration tests supply native-format stored preferences directly.

Successful catalog results run through disposable matching workers. New
watches begin at read position 0, and the subsequent thread refresh counts
their existing replies. Existing watches retain their label and read state.
Label preparation follows `ThreadWatcher.generateLabel` in the pinned
extension: select subject, otherwise collapse exact `<br>` runs and strip
comment tags, otherwise use `No.ID`, then slice at 45 UTF-16 code units. A
data-only parser inside the worker converts that small result to plain label
text. This replaces the reference's insertion of the label as HTML; entities
are decoded once, and markup cannot create elements or load resources in the
watcher. Empty visible labels remain empty when saved and restored.

Extension unwatch records `ID-board: 1` in `4chan-watch-bl`. Manual rewatch does
not remove that suppression, matching the native toggle. Catalog unwatch
retains its separate behavior and does not add a blacklist entry. A successful
board catalog prunes blacklist keys only when the ID is absent from that
catalog. Failed, invalid and unrequested boards retain their blacklist state.
The latter is a failure-handling exception to the native wholesale replacement:
an unavailable board or changed filter scope cannot silently forget an
explicit unwatch and later restore it through automatic matching.

Blacklist storage accepts at most 4,096 canonical keys and 196,608 code units.
Invalid state prevents automatic additions; an unwatch that cannot safely add
its suppression record retains the watch and shows the reason. Entries are
not evicted to make room for automatic watching. These are explicit security
bounds on otherwise unbounded native storage.

Catalog fetching, worker matching and thread fetching share a 60-second cycle
deadline. Catalog bytes reduce the thread phase's remaining 16 MiB budget.
Requests keep their 4 MiB response limit, two concurrent slots and launch
spacing, including the phase boundary. Individual worker and request deadlines
still apply. Thread fetch/body waits now settle on cancellation even when an
injected transport ignores abort; late bodies are cancelled without waiting
on an uncooperative cleanup promise.

The candidate commit checks the current enabled state, filter contents, watches
and blacklist under the existing Web Lock. Storage changes, disabling and page
exit cancel the cycle. Delayed commits cannot apply stale matching results.
Protected watch mutations also read current settings after acquiring the lock.

`localStorage` does not provide an atomic transaction across two keys. Unwatch
persists suppression before removing the watch; automatic discovery persists
additions before pruning proven-absent suppression. A failed write switches to
the existing same-tab fallback. An interrupted write can leave an older watch
or extra suppression in persisted storage, but this ordering does not leave a
persisted unwatch without its suppression. No database transaction or crash
atomicity is claimed for browser preferences.

The Rust server still serves the fixed filter bundle and the existing watcher
aliases. This integration adds no CSP source, database grant, migration or
production media permission. The editor, filter hide/highlight presentation,
remaining settings and post-menu fidelity, archive/expiry qualification and
full reference captures remain unfinished.

Integration checkpoint validation on the owned Windows/Chromium/PostgreSQL setup:

- `npm run test:behavior`: 85 unit tests and 74 browser tests passed. The browser
  groups contain 47 general, 13 watcher/transport, 4 lifecycle, 7 automatic-watch
  and 3 posting cases, each retaining its existing server request budgets.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p board-public --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test -p board-public --all-features --all-targets --locked -- --test-threads=1`:
  all 74 test executions passed, including database-backed cases.
- `VISUAL_FIXTURE_SERVER=1 npm run test:visual`: all three comparisons passed;
  no screenshot baseline changed.

The aggregate-budget browser test pads healthy owned JSON responses with
bounded JSON whitespace. Catalog traffic leaves insufficient capacity for all
three thread responses. A separate thread-only control then successfully reads
the same three padded responses with a full budget, distinguishing the shared
ceiling from malformed input or an unavailable service. Unit tests also cover
a staggered request whose remaining budget is consumed by another slot,
cancelled/hung transports, uncooperative cleanup, exact large IDs, empty labels,
blacklist limits, matching failure and invalid worker labels.

Initial browser failures came from an incorrect label expectation and reading
blacklist storage before the unwatch lock completed. The corrected tests assert
the actual source subject, plain rendered label and completed removal. A parallel
Rust build hit Windows access denial while the browser server held the public
executable; the sequential rerun passed. These checks qualify the integration,
not the unfinished editor or complete watcher parity. Hosted checks must still
qualify the committed head.

### Ordinary reply hiding

The pinned public extension v1191 `ReplyHiding` and `PostMenu.open` supply the ordinary reply-menu behavior: Hide post/Unhide post, `pc<id>.post-hidden`, `sa<id>[data-hidden]`, and the board-scoped `4chan-hide-r-<board>` map from exact post IDs to millisecond timestamps. Visiting a stored hidden reply renews its timestamp before unseen entries older than seven days are purged. Empty saves remove the key. OPs are not hidden through this path. The reference's embedded CSS hides reply content while retaining the post information; the release uses its existing hidden-post CSS and escaped server-rendered markup.

The bounded replacement accepts at most 512 IDs and 65,536 characters of stored data. IDs must be positive i64 strings; timestamps must be nonnegative safe integers no later than the current clock. Invalid storage remains untouched and posts stay visible with a warning. Competing same-board writes merge under a Web Lock; a queued action preserves the requested hide/unhide state rather than toggling another tab's newer state. Locks have a five-second acquisition deadline and at most 16 pending actions. Disabling or leaving the page cancels waiting changes. Unavailable storage or locking uses an explicit same-tab-only warning and reversible local state, never an unlocked persistent overwrite.

On reload, a matching Hide filter takes precedence over a stored manual reply hide, as established by `Parser.parsePost` and the true return from `Filter.exec`. That stored timestamp is not renewed and can expire. Highlight-only matches return false and still permit manual hiding and renewal. Restoration waits for bounded worker matching before choosing this precedence. Revealing a filter-hidden post does not silently restore its suppressed manual hide. An explicit Hide post action after loading can still hide that reply; its container state is separate from the child post's filter state. Cross-tab changes update open menu labels, and the global disable setting reveals replies without deleting preferences. No-JavaScript pages keep their visible replies and existing posting/report/deletion forms. `native-reply-hiding.test.mjs` covers exact IDs, malformed/bounded storage and timestamp expiry; `native-reply-hiding.spec.js` covers persisted menus, competing tabs, disable cancellation, mobile filtering interaction, Hide/highlight reload precedence and renewal, expiry, unavailable storage/locks and no-JavaScript rendering. Recursive reply hiding, thread hiding, and full native reference captures remain unfinished; this section does not claim complete menu or visual parity.
