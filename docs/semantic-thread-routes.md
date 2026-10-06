# Inbound semantic thread URLs

The public HTML listener accepts `/{board}/thread/{id}/{context}` for ordinary
source-generated word slugs. It reuses the canonical thread handler, storage
snapshot, visibility rules and response headers. The context is cosmetic and
cannot select a different post or board. HTML link emission is unchanged.

The supplied renderer emits these links in `imgboard.php:3616–3622,9317–9329`.
Its context generation and cleanup are at lines 7938–7978 and 8084–8110. The
extension identifies board, page and thread ID without using the context segment
at `extension.js:9369–9372`.

Accepted contexts contain lowercase ASCII letters or digits, joined by single
hyphens, with a maximum of 49 bytes. IDs retain exact positive signed-64-bit
precision. The handler and CSP classifier share raw-path validation, including
the board segment. Encoded syntax, extra/trailing segments, malformed IDs and
JSON keys are rejected. Contexts named `archive` or `catalog` still receive the
thread page's policy.

The database test compares complete headers and bodies with canonical responses,
including query behavior and IDs above JavaScript's safe-integer range. It also
checks missing/private targets and the rejected path forms.

The supplied files contain no original webserver rewrite configuration. Arbitrary
suffix acceptance and unusual control-whitespace generator output remain
unverified. This bounded inbound alias does not complete issue #216 or establish
full original-page appearance and client behavior.

## JSON context generation

OP JSON now derives `semantic_url` from the saved subject and formatted comment,
following `generate_href_context` and `cleanup_context_string` in the reference.
A usable subject takes precedence. Otherwise the source's ordered URL, markup and
first-line cleanup supplies the context. Punctuation is removed rather than
turned into word separators; whole words fit within the source's 50-byte running
budget, leaving at most 49 bytes in ordinary output. Empty contexts are omitted.
Replies and reduced-tail replies do not acquire the field.

Stored subjects are escaped once before source-style entity decoding. Saved
comment formatting, filter payloads and randomizer results supply the fallback;
current board formatting settings do not rewrite historical content. Older rows
retain their existing serializer semantics; missing original HTML or filter
provenance cannot be reconstructed. Fixed
regexes have explicit match, depth and heap limits. A formatting error fails the
projection rather than returning a partial context.

This changes JSON context generation only. HTML link emission still uses its
existing paths. Source cleanup can retain control whitespace inside a word;
that output is not accepted by the bounded ordinary-word alias described above.
The original rewrite configuration remains unavailable, so this edge is not
claimed as complete routing parity.

The JSON database regression checks both listeners and thread, index, catalog
and reduced-tail projections. It covers saved filters, dice and fortunes after
current board flags change, private-board hiding, empty fields and unchanged
stored posts. Pure tests cover entity spelling, whole-word boundaries, Unicode
URL bytes and malformed input.
