# Isolated replay candidate preparation

`apps/public/client/native-replay-data.js` is not served or wired into the runtime.
No served asset or application module imports or activates it. The asset build,
routes, CSP, editor and UI remain unchanged. Isolated Node tests import it, and
the package test scripts run those tests. There is no approved replay delivery
route or playback type. Neither API approves playback, storage, publication,
provenance, PNG correspondence, logical state, or resource cost.

## Two explicit boundaries

`decodeReplayCandidateWire(bytes)` accepts one already-acquired `Uint8Array`
(including a Node `Buffer` or a byte view from another realm). It makes a
bounded private copy and validates every record before creating deeply frozen
structural data. The result's `kind` is `untrusted-replay-candidate`.

- The representation is fixed `IBRPLY01`, version 1, candidate profile 1.
  Profile 1 names the representation, not an approved playback profile.
- Bounds are 2 to 16,384 events and exactly `256 + 16N` bytes, at most 262,400.
- All framing, count, metadata dimensions/tool ID, ordered tool IDs, flag bits,
  reserved/unused bytes, finite float32 fields, canonical boolean payloads,
  known event tags, and exclusive endpoint markers are checked.
- All 28 structural tags are represented. Event `timestampMs` remains u32;
  coordinates remain signed i16; pressure remains u16 when present. Tags 7/8
  have no `pressure` property. No normalization or fallback pressure is added.
- Float32 fields are read directly into JavaScript Numbers. Negative zero,
  subnormals, and all finite values are retained, even outside semantic ranges.
- Shared/resizable backing stores, proxies, non-byte views, and detached or
  out-of-size views are rejected. Intrinsic typed-array getters and copying avoid
  caller iterators, `slice`/species, and overridden `buffer`/length properties.
- Acquisition limits, streaming/backpressure, waiting for EOF, and deadlines
  must be enforced before this byte-slice API. Its limits do not bound prior
  buffering or any rendering/native work.

`prepareReplayCandidate(candidate, replayViewer)` requires that exact frozen
candidate from this module. A private WeakSet brand prevents forged objects,
clones, wrappers, or mutation of a separately validated input from bypassing
the structural check. The brand establishes only snapshot ownership.

The provider must be a trusted, unmodified production Tegaki 0.9.4 viewer from
the pinned source below. Executable provider identity cannot be authenticated
from a version string or from an arbitrary object supplied by a caller. The
module does not discover, create, initialize, or open a production viewer.

Preparation:

1. Rejects tags 13, 14, 17, 21, 22, and 23 before consulting the provider. These
   are the dynamics-change and delete/move/merge tags excluded by the current
   Rust core checker, including false dynamics-change values. This is only a
   tag-subset restriction; it does not duplicate or establish the core's other
   tool, setting, chronology, stroke, layer, history, or cost restrictions.
2. Calls `replayViewer.getEventIdMap()`, then checks and snapshots all needed
   own constructor entries before constructing any event.
3. Uses those actual source constructors. Color takes `(timeStamp, [r,g,b])`;
   pressure drawing takes `(timeStamp,x,y,pressure)`; no-pressure drawing takes
   `(timeStamp,x,y)`; scalar events take `(timeStamp,value)`; empty payloads
   take `(timeStamp)`. The adapter defines no dispatch implementations.
4. Returns frozen `metadata`, `toolMap`, and `events` with `kind` set to
   `untrusted-tegaki-preparation`. Metadata uses source property names, RGB
   arrays, and epoch seconds multiplied exactly by 1,000. Tool slots retain
   all eight IDs and numeric 0/1 source flag fields. `usePreserveAlpha` is a
   capability, not `preserveAlphaEnabled`.

Preparation does not hydrate or mutate the supplied viewer. It produces no
`loaded`, `playing`, duration, policy decision, state result, or cost result.
Candidate times can run backwards and other semantics can be invalid. The caller must
not infer playback authority from either successful API call.

## Ownership and future runtime work

All candidate data is deeply frozen and retains no caller-owned input bytes.
Prepared arrays, metadata, tool-map slots, and event own fields are frozen.
Event objects still have their real source prototypes; those prototypes and
the provider belong to the trusted pinned realm. Preparation neither changes
nor freezes shared runtime prototypes, and never calls their event methods.

The pinned `initToolsFromReplay` reads map entries and copies their values into
live tool objects. Event dispatch and stepping read event own fields. The
prepared own data can therefore be frozen without mutating the source's live tools.
This inspection is not lifecycle or rendering qualification, and does not
authorize handing this preparation to the current viewer.

There are no imports, fetches, source decompression, raw TGKR paths, dynamic
`eval`/`Function`, dispatch, UI, rendering, or source open/init/play calls in the
module. In particular, setting the old source compressed flag to zero is not
a safe alternative: the pinned old loader still inflates its payload.

Runtime support still requires independent host state and a chosen resource
policy, approved canonical delivery, exact consumer trust/versioning, a cold
viewer realm isolated from the retained editor, reconstructive reset/rewind,
generation-owned acquisition, correct single-chain RAF cancellation, and
coverage/removal of cursor, viewport, DOM, native/GPU, and lifetime costs.
Neither this preparation nor existing non-authoritative Rust checks provide
those guarantees.

## Independent tests and source oracle

Production source is the unchanged vendor `tegaki.min.js`, 110,619 bytes:

`daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744`

The helper `tests/helpers/pinned-replay-runtime.mjs` checks that whole-file pin,
loads it in a fresh Node VM with dynamic code generation disabled, and uses a
literal inert setup to access the actual lexical viewer class. It never calls
`Tegaki.open`. During bootstrap, the only allowed DOM access is an inert
`document.documentElement` reference. Network, DOM, timing, canvas and image
boundaries throw, as do viewer lifecycle/load/decode methods and event
dispatch/pack/unpack methods.
Guard replacements do not reimplement any constructor semantics.

`tests/fixtures/native-replay-source.json` freezes own fields and constructor
names obtained from real source constructors using an independent literal
transcript of the commands recorder fixture. The recording helper imports neither
adapter nor decoder and never reads or inflates TGKR. All 28 tags appear in the
oracle, including those preparation rejects. Existing independent
`tests/media/fixtures/replay-wire/{empty,commands}.ibr` bytes are SHA-pinned;
tests compare structural values and prepared objects to the transcript and
source results rather than only to a locally implemented encoder.

Run the aggregate from the repository root with Node 24 or newer and Python 3:

```sh
npm run test:replay-foundation
```

It verifies production recorder fixtures from the unchanged vendored source,
independent wire transcriptions, extraction of frozen source cost counters,
the constructor oracle, and the Node adapter tests. Bounded temporary-copy
controls exercise source pinning, same-length mutations, frozen-output drift,
malformed CLI and record mode; Python integrity checks are exercised with
`PYTHONOPTIMIZE=0`, `1` and `2`. These controls do not edit production source or
checked-in fixture bytes. No external reference checkout is needed.

The existing `test:drawing-core` script retains its checks and then runs this
aggregate. Linux CI reaches it through `scripts/verify.sh` → `test:drawing` →
`test:drawing-core`; Windows `visual-windows` directly runs
`test:drawing-core`. This is test-only qualification, with no replay runtime
activation. It does not rerun the separate external source instrumentation,
state or cost oracle suites, and does not replace Rust tests.

Individual constructor-oracle and adapter checks remain available:

```sh
node tests/fixtures/record-native-replay-source.mjs
node --test tests/browser/native-replay-data.test.mjs
```

Use `--record` on the oracle helper only for a deliberate fixture replacement.
Tests cover malformed/truncated/extra/maximum inputs, every tag and unused
payload layout, tool flags and ordering, nonfinite values, exact float/pressure
semantics, metadata/tool-map shapes, mutation/forgery, missing constructors,
and forbidden-path traps. They do not exercise event dispatch, raster output,
source lifecycle, a real browser, or a delivery/approval path.
