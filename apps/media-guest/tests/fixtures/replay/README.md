# TGKR structural fixtures

These synthetic fixtures come from the unmodified production `js/tegaki.min.js` in
the supplied 4chan-old snapshot at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The generator checks the whole source
file's SHA-256 before executing it in a Node VM with inert UI stubs.
It uses the real recorder, event constructors, binary writers, and compressor.
It never dispatches events or paints a canvas.

- `empty.tgkr`: default metadata, all eight tools, prelude and conclusion.
- `commands.tgkr`: all 28 known tags, tool settings, signed coordinates, pressure
  extremes, mixed pressure encodings, layer commands, and undo/redo.
- `manifest.json`: source pin, fixture digests, event tags, timestamps, and sizes.

From the repository root, verify the files without rewriting them:

```sh
node apps/media-guest/tests/fixtures/replay/generate.cjs /path/to/26chan-reference
cargo test -p board-media-guest --locked --test replay
```

The reference-root argument is optional. Without it, the generator uses the
identical, already-vendored
`apps/public/vendor/tegaki/0.9.4/tegaki.min.js`. Use
`--source-file /path/to/tegaki.min.js` to select a source file instead of a
reference root. The two selectors cannot be combined. The CLI, exact 110,619
byte length, bounded file read, and whole-file SHA-256 are checked before VM
execution. The source is never rewritten.

Append `--record` only when deliberately replacing fixtures. Record mode still
requires the same source pin and generates all outputs before writing any of them.
Unknown, repeated, missing, and conflicting arguments fail closed. The normal
Rust tests use saved bytes and need neither Node nor a reference checkout.

`npm run test:replay-foundation` verifies the recorder outputs from the vendored
source, independent wire transcriptions, frozen cost-counter extraction,
constructor oracle, and isolated Node adapter tests. It also runs bounded
temporary-copy pin, CLI, frozen-output and record-mode controls. The existing
Linux and Windows drawing-core CI paths both invoke this package script; no
external reference checkout or independent private oracle suite is required.

The command stream uses synthetic recorder input; no browser UI was exercised.
These fixtures establish wire-format expectations only. They do not establish
valid history/layer semantics, safe playback, rendering cost bounds,
or correspondence to an uploaded PNG. The parser's stricter version, length,
boolean, tool-table, marker, and resource checks intentionally differ from the
original PHP validator and JS viewer.
