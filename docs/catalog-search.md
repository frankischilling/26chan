# Catalog search operator and case contract

Inspection of the pinned public v1025 client found a narrower escape list than
the earlier description of literal filtering implied. Function `o` escapes
slash, period, repetition punctuation, parentheses, brackets, braces and
backslash. It does not escape `^`, `$` or `|`. Function `p` constructs a regex with
the `i` flag, not `u` or `m`. The source hash and observed list are recorded in
`public-catalog-reference.json`.

The server now implements those three operators and literal alternatives without
adding a general regex engine. For example, `^Alpha` means a prefix, `fold$` means
a suffix, and `crane|boat` accepts either word. `[.*]` and `(a+)+` remain literal
text, not character classes or repetition. An empty alternative matches any
text. Anchors do not acquire multiline behavior.

Case comparison follows the non-Unicode ignore-case branch of
[ECMAScript Canonicalize](https://tc39.es/ecma262/multipage/text-processing.html#sec-runtime-semantics-canonicalize-ch),
retrieved September 13, 2026. It compares UTF-16 code units using uppercase
mapping, retaining units whose mapping expands or would turn non-ASCII into
ASCII. This differs from lowercasing whole Unicode strings. The shared cases
include Greek sigma, dotted/dotless I, long S, Kelvin and Ohm signs, sharp S,
accented Latin letters and supplementary-plane characters. Unicode table updates
still require running the cross-runtime checks; this is not a claim covering
every historical browser version.

## Bounds and remaining differences

Queries remain limited to 128 decoded characters, without control characters,
and 2048 raw query bytes. Compilation accepts at most 129 alternatives and 256
UTF-16 literal units. Matching uses bounded literal comparisons, not recursive
backtracking, nested repetition, lookarounds or backreferences. The worst-case
literal comparison work is proportional to input units times the bounded total
query units. Existing public admission and handler deadlines remain in force.

The server still searches subject, raw comment and non-deleted display filename
as separate fields. The reference client searches its generated teaser and file
field, whose server preprocessing is not established by the permitted source.
Field selection is therefore still a documented difference. [Live filtering](catalog-live-search.md)
now uses the same bounded pattern contract with the observed 250 ms debounce,
per-tab storage keys and fragment links. The original search-toggle interface and
complete generated-teaser parity remain unfinished. GET is retained as fallback.

No HTML rendering, CSP, database privileges, dependency, media or deployment
policy changes are included. Production readiness and full search parity are
not established by this contract change.

## Checks

`tests/fixtures/catalog-search-cases.json` contains shared operator and case
examples. Rust unit tests and the pinned Chromium browser execute the same
expectations. Bounded property tests check literal/anchor behavior and code-unit
canonicalization; compilation-bound tests cover rejection and the maximum number
of alternatives. Real database-backed catalog handlers test prefix, suffix and
alternative queries alongside escaped output, deletion and visible ordering.

`node scripts/verify-public-catalog-search.mjs .local/reference/catalog-20260913`
checks the pinned client hash, reads its JSON escape-character array without
executing the full client, and verifies the shared cases against the corresponding
native JavaScript regex. Run that verifier, public Rust tests and
`npm run test:behavior` before accepting a reference or toolchain update. Existing
visual baselines should remain unchanged.
