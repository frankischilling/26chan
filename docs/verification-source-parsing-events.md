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

## Remaining gap

`4chanMainInit` is not implemented. Source `Main.init` emits it after setting
board/thread context and before parser startup (line 9374). The native math
module can mount and transform comments on import, before watcher startup
finishes establishing settings. Emitting MainInit late in the watcher would
claim a pre-parser ordering that is not true on math-enabled pages. A separate
bootstrap reordering needs regression coverage before exposing that event.

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
