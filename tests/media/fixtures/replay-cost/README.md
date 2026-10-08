# Non-authoritative replay resource-model evidence

These fixtures record observations from independently instrumented Tegaki source.
They are not approved replays, browser measurements, production budgets, or
estimates from the Rust module under test. The host borrows a
`StateCheckedCandidate` without drawing, mutating it, executing source JavaScript,
or acquiring authority.

## Provenance and reproducibility

Production source: Tegaki 0.9.4, source revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`, `js/tegaki.min.js`, 110619 bytes,
SHA-256 `daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744`.
The independent source suite checks that pin before decoding, transforming, or
evaluating source. Its 47 exact, nonoverlapping counter insertions produce
SHA-256 `46e2e650f715ba9322d86a5b264f71a1739e6e6ade272766c8f3b1d19eb547ae`.
Separate execution of the original source confirms the tested kernels and
brush pixels.
The source's event, kernel, tone, history, layer, and pressure bodies remain
intact. Canvas methods are inert request recorders, not native rasterizers.

`source/*.json` are byte-for-byte frozen output from the separately reviewed
`26chan-replay-cost-oracles` suite. The extension has six scenarios and 47
measurement windows, including real constructor/header initialization, actual
creation-only layer insertion, Draw/Alpha/Dummy history, alpha coalescing with
redo preserved, pending aliases surviving eviction, and optional flatten.
Creation tests preserve the source's real layer-grid and preview-sizing functions;
the rest of the UI remains inert. The nominal viewport cursor is excluded.

The independent source suite's frozen generator/runtime pins are:

| File | SHA-256 |
| --- | --- |
| instrument.cjs | 67167da4abd38f7938c1867db3353b25936df5f0b64d0f850056738b50663e83 |
| oracle.cjs | a3077f64fefa2f96d48603174afd9024fef88a04c2c847d8c1db0764430da00b |
| runtime.cjs | f670d4ac228f36a0d4dbb7089adea6f7f0ecfaa51219cde51c2647819e73e3d8 |
| state-cost.cjs | 1fde5e70cbe4d40acdc8d96457a5ac7e541544a21bc059f87d0a7e7f9814910f |
| ledger.cjs (proposal, not authority) | 0f69b36438719cbacc1c1477709070cd3f17c54c4cdbe8380222ea0ae0722cc3 |
| README.md (brush proofs) | 873d3037456d77ffd695a90114c542436cf1aa6a553c5ec1a2a23a050172a06d |
| STATE-COSTS.md (extension proofs) | 4fee82eb619e5771de3d14313b31084b89650593f2f4ff588db6c9f9b8e57886 |

The raw JSON SHA pins are hard-coded in `generate.py` and the Rust test, which
also pins the extractor itself and every generated text fixture. The extractor
checks hashes before JSON decoding, including under `python3 -O`.
Run from the repository root:

```sh
python3 tests/media/fixtures/replay-cost/generate.py
python3 -O tests/media/fixtures/replay-cost/generate.py
```

Use `--record` only to replace the extracted text fixtures deliberately; it cannot
bypass input pins. The extractor imports neither the host estimator nor
`ledger.cjs` and calculates no cost envelopes. It exports actual source counters,
sums raw measurement windows, and transcribes synthetic source event inputs.
The raw `ledger.json` includes the original proposed estimate for auditability, but the
extractor reads only its plan and `measuredSourceCounters`; tests never use the
proposal's estimate as an independent expected answer. Normal Rust tests need
no Node/Python, source checkout, network, or browser.

`npm run test:replay-foundation` checks this frozen extraction and bounded
temporary-copy input-pin, CLI, output-drift and record controls under
`PYTHONOPTIMIZE=0`, `1` and `2`. It is reached by the existing Linux and Windows
drawing-core CI paths. These checks establish reproducibility only for the
committed raw JSON and extracted text. They do not rerun or establish CI coverage
of the separate source instrumentation, state or cost oracle suites described above. No
external oracle directory is required or copied into the repository.

Fixtures:

- `shapes.tsv`: all 512 source tool/tip/size combinations, actual generated side,
  typed bytes, defined shape-work units, and scalar stack pushes.
- `segments.tsv`: all 672 source brush segments, endpoints, actual kernel side,
  traversal points, measured leaf units, and measured selected loop units.
- `tools.tsv`: all eight actual source constructor/header slots.
- `ledger-events.txt` and `ledger-counters.tsv`: the independently executed
  190-event, 60-stroke source plan and raw cumulative source counters.
- `state-scenarios.tsv` and `state-counters.tsv`: the six extension scenarios
  and 26 extracted counters each. The flatten scenario compares only its final
  flatten window; its two unmeasured Add setup events and visibility toggle
  are explicitly transcribed from pinned `state-cost.cjs`.
- `tone.tsv`: 18 source setting/stroke windows, retaining actual first-map and
  reuse observations. Some source windows (generic non-eraser tip) are outside
  the stricter state profile and are not treated as accepted host inputs.

## Model units and analytic mapping

Let `A=width*height`, `s=size`, `H=4(s+2)`, and rendered brush side `B=s` for
pencil/tone/blur/eraser-pencil, `s+2` for pen/eraser-pen, `2s` for airbrush or
eraser-airbrush. Sizes are 1..64; all dynamics are disabled. For a segment let
`dx=abs(x1-x0)`, `dy=abs(y1-y0)`, `I=max(dx,dy)+1`, `Q=dx+B`, `R=dy+B`.
Valid integer endpoints and initialized position are established by state
checking. The source processes the terminal traversal point too. At most one
stamp occurs at each point; the model never assumes that spacing saves cost.

Each regeneration is charged `64H²` defined instrumented shape-work units,
`12H²` typed backing bytes, and `4H²+2` flood-stack scalar pushes. Each flood
pixel is successfully visited once; a visit pushes at most two coordinate
pairs. Popped pairs are at most `2H²+1`, including the initial pair. The shape
work envelope includes the instrumented flood/circle/downsample work, not all
JS operations. Pencil requests at most `8s²` bytes, pen `8H²+4(s+2)²`, and
airbrush `4(2s)²`, each within the common backing envelope. Only the initially
selected tool regenerates during setup. Every later accepted selection, size,
and eraser-tip setter regenerates, even if its value is unchanged. Tool size
and eraser tip are remembered separately per tool.

Ordinary brush leaf work is `I+IB²`; blur leaf work is `I+10IB²+2QR`. Selected
loop work is separately `I+I(2B²+3B+1)` for ordinary tools, and the corrected
`I+I(30B²+3B+1)+4QR+6Q+2` for blur. For Start, use `I=1,Q=R=B` and omit the
traversal term. Rectangle copies are not clipped to visible canvas pixels.
The size-1 blur source counterexample has segment leaf work 107 and selected
loop work 373: the respective envelopes are 117 and 407. Calling 117 a bound
on all these loop bodies/tests would be incorrect.

The first tone selection builds all 16 maps, even with alpha zero: `16A` new
bytes, `16A` map-cell bodies, and `32A+48*height+49` selected map/row/cell
loop units. Later alpha changes and switches reuse them at fixed dimensions.
Pressure/no-pressure/mixed inputs receive the same resource envelope with
dynamics disabled; the source's unusual `.5/65535` no-pressure meaning remains
untouched by this module.

| Operation | Cumulative typed backing | Explicit copies/clears | Other requests |
| --- | ---: | --- | --- |
| Initialization before shape | 12A | none | background+layer nominal 2A, one preview ≤576, background fill A, layer get A |
| Add | 4A | none | layer surface A, preview ≤576, get A |
| Start | 4A | copy 4A | brush work, put with A input and B² dirty request |
| Draw | none | none | brush work, put with A input and QR dirty request |
| Commit | 4A | copy 4A, clear 8A | preview source A and destination/clear ≤576 |
| Every Undo/Redo | 4A | copy 4A | full put A and preview |
| Explicit optional final flatten | none | none | nominal new surface A, background plus each visible layer full-area draw |

Every Undo/Redo is conservatively charged as Draw restoration. Actual
Alpha/Dummy restores have zero counted raster/typed backing costs, so this is
an intentional overcount. Metadata, selection, and visibility events still
count toward event overhead frequency; their CSS/UI/JS cost is not assumed to
be zero. No allocation is refunded for history eviction, aliases,
replacement, undo/redo movement, or garbage collection. Conclusion does not
flatten. Flatten is a distinct, explicit caller choice, charged once.

The report separates cumulative total, initialization, component-wise peak
event, and optional final-flatten counters. A peak excludes initialization and
flatten, and different components can peak at different events. One size-64
regeneration is 4,460,544 defined shape units, which is not interchangeable
with a proposed one-million-unit draw-event threshold.

## What passing means

Tests compare raw source observations against the independently implemented
host envelope. They also check boundary/overflow rejection, repeated settings,
metadata events, 100 Draw restores, source-preserved redo, source-visible
flattening, fixed tool state, tone reuse, maximum event count, no-pressure
variants, zero-alpha work, and immutable input. Overflow tests directly probe
internal checked arithmetic because current bounded valid inputs cannot reach
`u64::MAX`; no invalid candidate is forged to claim otherwise.

Passing explicit `CostLimit` values means only that those numeric model comparisons
passed. No default budgets exist, and an empty limit list checks nothing. Each
limit names a metric, scope, and inclusive maximum; a failure reports the
actual metric/value. No unit mixing, approval type, runtime caller, image
agreement, safety certification, or replay publication is provided.

There is no bound here on total JS/DOM/array storage, real UI/CSS rendering,
cursor or viewport-dependent allocation, native canvas or GPU work/memory,
GC/JIT, decoding/acquisition, concurrency, external events, replay seek/rewind,
or actual time/RSS. Nominal surface pixels and Canvas API request volumes are
not native-memory estimates. Consumer equivalence and deployable numerical
policy need separate qualification.
