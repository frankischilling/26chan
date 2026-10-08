# Drawing annotation oracle evidence

These six JSON files are byte-for-byte copies of the independent local
`26chan-replay-oracles` bundle, frozen on 2026-10-08. They were not generated from
the Rust implementation. `cases.json` and `source-pins.json` came from the bundle
root; the other files came from its `fixtures/` directory. Preserve the original
bytes and review any proposed replacement. The Rust integration test pins each
file's SHA-256, so changing an expectation cannot silently update the oracle.

The source reference revision is
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The executed source is
`lib/oekaki.php:102–160` and the `imgboard.php:6063–6097` block. The earlier
accepted-image guard is an explicit synthetic harness input. `source-pins.json`
records whole-file and excerpt SHA-256 values, including original line endings.
`runtime.json` identifies the independent PHP 8.3.6 / PCRE 10.42 execution.

## Rust comparison scope

`drawing_annotation_reference.rs` compares 61 frozen outcomes:

- 29 integer-typed time and display-intent cases, including `60m`, `1h 60m`, the
  inclusive range of 1 to 5,184,000 seconds, and source/replay precedence
- 16 source-ID cases using supplied synthetic records from both boards
- 16 successful post-upload annotation cases, including replay optionality,
  replay storage without annotation, filename independence of annotations, and
  source references suppressing only the replay link

Only the test parses the original returned HTML, to compare time text and typed
link intent. Rust emits no HTML, JavaScript, URL or replay identifier.
This is not renderer qualification.

The API accepts parsed integers; it has no form-string parser or PHP coercion
layer. Eight raw time/coercion cases, four raw source-ID/coercion cases, the
fractional posting-time case and three upload/storage hard-error cases are
excluded by name from Rust parity assertions. Their original data remain in the
frozen files. Arrays, objects, PHP warnings and runtime errors are not ported. The three source query-failure/row-count cases are also
outside the pure supplied-record predicate. A future database adapter must
resolve genuine, unambiguous selected-board records and handle lookup failures.

The fixture bundle also contains original cases that this module does not
exercise. Including them does not establish replay validation, recorder or
browser coverage. No external oracle directory, PHP runtime, database or source
checkout is required for the ordinary Rust tests.

## Boundaries

Submitted annotation time is an independent wall-clock value. It does not prove
the replay duration, header interval, drawing provenance or time actually spent
drawing. Eligibility of a supplied source record does not prove its image was
used, that its file is readable, or even that the target thread exists. The
original `tim != 0` predicate deliberately does not check `filedeleted`; future
image/replay read authorization is separate.

`has_stored_replay` and accepted-image facts are input observations, not upload
admission or storage decisions. Missing or invalid annotation time, or a valid
source reference, cannot mutate storage. Replay remains optional. There are no
new schema fields, posting routes, browser handlers, or production call sites.

## Local verification

Initial verification included the eight Rust tests, a byte-for-byte comparison
of all six files with the independent bundle, and rechecks of all six source-file
and five excerpt pins. Unchanged original PHP was re-executed against all 37 time
cases, 23 source-function cases and 20 posting cases and matched the frozen outcomes.
The source-function rerun used already-frozen synthetic row counts as inert
stubs; it did not execute SQL or test a live database. An additional original-PHP
probe confirmed that present but invalid time still resolves a supplied source
before suppressing the annotation. A Rust presence-ordering test covers this
observation separately from the 61 frozen comparisons.

Run the repository-local comparison with:

```sh
cargo test -p board-domain --test drawing_annotation_reference --locked
```

The complete `board-domain` test suite and Clippy should also pass. These checks
do not establish storage integration, genuine upload provenance, SQL adapter
behavior, browser rendering/playback, or end-to-end drawing parity.
