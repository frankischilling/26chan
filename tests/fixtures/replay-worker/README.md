# Test-only native worker replay differential

This qualification experiment does not enable a production runtime, route, asset,
CSP permission, setting or upload path. Native browser tests were not run locally.
The Node controls and exact-corpus Rust state/cost checks passed. The dedicated
hosted workflow must pass before claiming native rendering results.

## First proof and current scope

The first proof target is `pencil-pressure`: a fresh 24×24 original-DOM Tegaki
realm versus a fresh dedicated worker, with one paired encoded-pressure stroke.
Each initialization/event boundary separately compares:

1. Authoritative per-layer source `ImageData` bytes.
2. Native layer canvas `getImageData` readback, retaining any premultiplication loss.
3. Core layer/order/active/selection/tool/pressure/buffer/history state, including
   actual kernel/cache values, history snapshots and pending-action aliases.
4. Output of the original source `flatten` implementation.
5. Canvas readback after the parent draws the received native `ImageBitmap`.

Matching authoritative arrays does not stand in for native readback. The parent
canvas readback does not prove CSS compositor, zoom, device-scale or playback
scheduler/timing equivalence. Those are explicitly unclaimed.

The 18 fixed transcripts also cover all six admitted tools, three eraser tips,
alpha/flow, preserve-alpha, zero alpha/flow, source no-pressure events, repeated
settings, pen size 64, edges, tone initial/later selection, asymmetric blur,
1×1 geometry, middle layer insertion, eight layers, visibility, selected alpha,
Draw/Alpha/Dummy undo/redo, alpha coalescing with retained redo, and history
50-action eviction. Coverage is representative, not full qualification. Encoded pressure and the
source no-pressure divisor are checked as state. Size/alpha/flow dynamics remain
disabled and excluded by core-v1: these cases do not qualify pressure-modulated
rendering or enabled dynamics.

## Independent assertions and negative controls

The two realms intentionally share the canonical preparation, dispatch wrapper
and inspection schema. Differential agreement alone can therefore miss a shared
wrapper error. `semantic-checkpoints.mjs` adds separately authored literal
expectations, without importing the case encoder, decoder, generated consumer or
adapter. Both realms independently have to satisfy them.

Representative checkpoints are grounded in the pinned source methods:

- `TegakiPencil.generateShape`, `TegakiBrush.start/brushFn` and
  `TegakiEventDrawStart`: size-one opaque start pixel `[43,61,79,255]`, unchanged
  background `[240,246,251,255]`, pressure division by 65535, pending Draw before
  commit, and replay start leaving `isPainting` false.
- `TegakiEventDrawStartNoP`: literal `.5 / 65535`, rather than normalized .5.
- `TegakiPen.easeFlow` and `TegakiTone.setAlpha/generateMapCache`: applied flow
  easing and the initial 16-map tone cache.
- `TegakiLayers.addLayer/setActiveLayer/selectedLayersToggle`: explicit middle
  insertion order `[1,4,2,3]`, zero/top-layer behavior, and selection order `[3,2]`.
- `TegakiHistoryActions.SetLayersAlpha` and `TegakiHistory.push`: earliest alpha
  restoration, retained Draw redo under coalescing, and pending Draw surviving
  50-action eviction with no remaining stack alias.

The four browser comparison-negative cases first establish an equal real-native
baseline, then flip one byte in a copied worker observation in exactly one of:
authoritative layer bytes, native layer readback, source flatten, or received
bitmap readback. Each must fail its named comparator. A fallback mismatch in
another comparator does not count. Live pixels and source code are untouched.
Node versions exercise assertion plumbing with synthetic data only.

The direct-native-worker protocol cases instantiate the real `worker.mjs`
without `ProbeController`. Missing, wrong, stale and duplicate acknowledgements;
stale, skipped and duplicate command sequences; premature step; duplicate
initial; and an extra message field must reach the real worker and fail closed.
The valid exact-ack control must release exactly one next event. Every received
native bitmap is closed, including unexpected frames. Post-error continuation is
checked in a bounded observation window, not claimed as a hard process guarantee.

These measures reduce shared-wrapper blind spots but do not create a fully
independent raster oracle: full complex brush outputs, non-checkpoint states and
all inspection fields still rely on shared source/wrappers. The literal
checkpoints and all native negative controls remain unexecuted here until the
hosted browser lane runs. They do not establish full-source or CSS parity.

## Source/adapter boundary

The original Tegaki 0.9.4 source is exactly 110,619 bytes, SHA-256:

`daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744`

`build.mjs` verifies this whole-file pin, copies its bytes as the unmodified
prefix of `generated/consumer-v1.mjs`, preserves license notices, then appends
`consumer-adapter.inc.js` in the same module scope. `generated/manifest.json`
separately records the adapter, canonical helper, generated consumer and corpus
hashes. The generator's default mode checks; `--write` is deliberate regeneration.
No brush, layer, history, pressure, event constructor, or dispatch body is
extracted or rewritten. No dynamic evaluator or general method dispatcher is
exported. Source UZIP tables still initialize as part of whole-source evaluation;
the source TGKR/URL loader and scheduler are blocked in both realms.

`worker.mjs` first installs the document boundary, then dynamically imports the
fixed generated module. A static source import before that installation would
be incorrect because source evaluation captures `document.documentElement`.

The reference uses original `Tegaki.open({ replayMode: true, saveReplay: false })`
in a fresh iframe, preserving real DOM/UI/cursor/previews. Both sides assign
only prepared metadata and all eight tool-map slots, then call the original
`initFromReplay → init → setTool(defaultTool)` sequence. The worker starts the
corresponding fresh core with its bounded containers and source viewer, without
building the interactive UI. Recorder stays null; no source RAF playback starts.

### Exact omissions

The worker replaces 15 source-proven output-only UI hooks:
`onToolChanged`, `updateToolSize`, `updateToolAlpha`, `updateToolFlow`,
`updateToolDynamics`, `updateToolShape`, `updateToolPreserveAlpha`,
`updateLayersGridAdd`, `updateLayersGridActive`, `updateLayersGridVisibility`,
`updateLayersGridSelectedClear`, `updateLayersGridSelectedSet`,
`updateLayerAlphaOpt`, `updateLayerPreview`, and `updateZoomLevel`.

The original `updateUndoRedo` remains; it already exits in replay mode.
Unlisted UI functions throw. Cursor `init`, `Tegaki.updatePosOffset`,
`Tegaki.bindGlobalEvents`, and `Tegaki.updateCursorStatus` are omitted together.
Original mixed setters, including `onToolChanged`, `setToolColor`, active-layer
selection and selected-alpha/history methods, are unchanged.

The worker's canvas factory creates genuine native `OffscreenCanvas` objects.
Metadata is attached directly; a guarded intermediate prototype retains native
identity/receivers and fails unknown properties. Native contexts and ImageData
are never replaced. Unknown DOM tags, IDs, class queries, style keys, methods,
and UI calls fail closed. An instrumented count map records audited sink access.
Native extensibility and the guarded-prototype technique are themselves
unqualified until the real browser suite passes; failure must not fall back to
mock pixels or another execution realm.

## Input and logical qualification

Only the exact named, hashed owned corpus can be loaded. Its dimensions are at
most 24×24 and it contains at most 96 events per replay. It is synthetic canonical
`IBRPLY01`, not an uploaded TGKR. The shared canonical helper provides its normal
structural and constructor checks, which do not establish state or cost approval.
The Node suite adds limited transcript sanity checks; it is not the Rust model.

The opt-in `rust-check` mini-crate runs the existing actual `decode`,
`check_core_v1`, and `estimate` interfaces against every exact hashed fixture.
It has its own workspace so it is not silently added to production Cargo CI.
Its exact-corpus assertion passed locally against all 18 fixtures. The combined
native qualification command below runs it again before browser tests. No fixed-corpus gate,
state check or cost result is general host admission or playback authorization.

## Presentation and lifetime ownership

The source `flatten` writes a separate retained presentation surface, whose width
and height it resets on every call. Only that surface calls
`transferToImageBitmap`; no live layer/history/ghost/blend buffer is transferred.
Exactly one command/frame is outstanding. The worker will not dispatch another
event until the exact generation/sequence acknowledgement arrives.

The parent draws and closes accepted bitmaps, closes stale/rejected bitmaps,
terminates the previous worker on replacement/close/error, and rejects pending
work. Startup protection is installed before Worker construction. A command's
parent-owned deadline stays active while its frame acknowledgement is withheld;
progress messages do not extend it. Restart count and per-replay event/frame
counts are bounded. This test controller has no autonomous idle RAF production.

Every initialization and every event is deliberately presented for differential
inspection. Repeated flatten/readback/bitmap transfer/parent display costs are
**outside the current one-time-final-flatten estimator**. Diagnostics also clone
bounded arrays. This is not a deployable resource policy, total CPU/RSS/native or
GPU memory guarantee, or evidence of immediate reclamation after termination.
The synthetic finite two-second stall tests check actual parent termination in
startup and command work; they do not measure expensive brush termination. A separate
browser case injects a retired-generation callback with a native bitmap to verify
actual bitmap close behavior, without claiming delivery after termination.

## Files and API

- `cases.mjs`: owned transcripts and test-only wire encoder
- `build.mjs`, `generated/manifest.json`: source/generation integrity
- `consumer-adapter.inc.js`: sole source-internal setup/dispatch/inspection API
- `worker-boundary.mjs`: closed DOM/UI sinks with native canvas identity
- `worker.mjs`, `protocol.mjs`: ordered import and command/frame/ack ownership
- `raw-worker-protocol.mjs`: direct native Worker integration-negative controls
- `comparison.mjs`, `semantic-checkpoints.mjs`: isolated comparison negatives and independent literal checkpoints
- `controller.mjs`: `start`, `initial`, `step`, `acknowledge`, `close`
- `index.html`, `reference.html`, `main.mjs`: standalone owned UI and fresh DOM reference
- `serve.mjs`: read-only allowlisted fixture server, no application or database
- `tests/replay-worker/preparation.test.mjs`: Node/source/control checks only
- `tests/replay-worker/native.spec.js`: 41 genuine-browser differential/control tests
- `playwright.replay-worker.config.js`: opt-in Chromium lane only

## Commands and recorded verification

Run from the repository root after sourcing the normal environment:

```sh
npm run test:replay-worker-probe-core
npx playwright test --config playwright.replay-worker.config.js --list
```

Recorded local checks: 17 Node tests, 41 browser tests discovered. The exact-corpus
Rust assertion also passed against the real existing state/cost interfaces. All probe JS
syntax checks passed. Discovery does not execute browser test bodies.

In a capable browser environment, the explicit opt-in qualification is:

```sh
npm run test:replay-worker-probe
```

It runs generated/source checks, Node tests, the genuine Rust core/cost corpus
assertion, and then native Playwright. The Rust mini-crate uses a checked-in lockfile whose registry packages match the
existing root lockfile. Its runner uses --locked --offline and keeps build output
in a separate system-temp target directory; REPLAY_PROBE_CARGO_TARGET_DIR can
select another target outside the checkout. Normal Cargo dependency setup may be
needed first in a new environment. Browser
engines must be installed using that environment's approved setup. The fixture
server is started and owned by Playwright, with no existing server reused.

Manual fixture viewing is available via `node tests/fixtures/replay-worker/serve.mjs`
in a permitted environment. Only the dedicated config discovers these browser
specs; existing application/visual CI and production test scripts are unchanged.

Still open: the first native pass, further initial-tool
and size/float extremes, source context-loss behavior, retained-editor coexistence,
delayed acquisition, termination during expensive real events, genuine CSS
composition, Firefox/WebKit/mobile/device matrices, and a consumer-specific
presentation/lifetime resource policy. None is implied by local Node success.

## Hosted qualification

The separate Native replay qualification workflow tests the exact candidate head
on Ubuntu 24.04 with the lockfile's Chromium revision. It runs the source, Node,
and real Rust corpus checks before all 41 native cases. A separate report gate
requires exactly one passing attempt for every case, with no skipped, flaky,
expected-failure or retried cases. Thirteen report-gate tests cover valid and misleading results.

Setup and test steps have individual deadlines within the job deadline, leaving
time to retain exact source identities, setup/test logs, results and synthetic
failure traces for three days. Artifact retention remains best-effort if the
runner is lost or the job is cancelled. No credentials, caches or build targets
are included. Passing this workflow qualifies only the checked Chromium
experiment; it does not authorize production playback or establish wider
browser, presentation, or resource guarantees.
