# Semantic thread URLs

The public HTML listener accepts `/{board}/thread/{id}/{context}` for ordinary
source-generated word slugs. It reuses the canonical thread handler, storage
snapshot, visibility rules and response headers. The context is cosmetic and
cannot select a different post or board, so stale context words still reach the
same thread.

The supplied renderer emits these links in `imgboard.php:3616-3622,9317-9329`.
Its context generation and cleanup are at lines 7938-7978 and 8084-8110. The
extension identifies board, page and thread ID without using the context segment
at `extension.js:9369-9372`.

Accepted contexts contain lowercase ASCII letters or digits, joined by single
hyphens, with a maximum of 49 bytes. IDs retain exact positive signed-64-bit
precision. The handler and CSP classifier share raw-path validation, including
the board segment. Encoded syntax, extra/trailing segments, malformed IDs and
JSON keys are rejected. Contexts named `archive` or `catalog` still receive the
thread page's policy. The alias is HTML-only; it is absent from the API listener.

## Shared JSON and HTML context

OP JSON `semantic_url` and the plain OP `Reply` link use the same projection of
the saved subject and formatted comment, following `generate_href_context` and
`cleanup_context_string` in the reference. A usable subject takes precedence.
Otherwise the source's ordered URL, markup and first-line cleanup supplies the
context. Punctuation is removed rather than turned into word separators; whole
words fit within the source's 50-byte running budget, leaving at most 49 bytes
in ordinary output. Empty JSON contexts are omitted. Replies and reduced-tail
replies do not acquire the field.

Stored subjects are escaped once before source-style entity decoding. Saved
comment formatting, filter payloads and randomizer results supply the fallback;
current board formatting settings do not rewrite historical content. Older rows
retain their existing serializer semantics; missing original HTML or filter
provenance cannot be reconstructed. Fixed regexes have explicit match, depth and
heap limits. A formatting error fails the projection rather than returning a
partial context.

The shared post template emits `/{board}/thread/{id}/{context}` for the OP's
`Reply` link, or its `View thread` link when archived. This covers board and
thread pages, depager snapshots, updater and one-post fragments, and search
results. HTML appends the context only when the complete path passes the inbound
alias validator. An empty or unsupported context falls back to
`/{board}/thread/{id}`. In particular, source cleanup can retain control
whitespace inside a word: JSON preserves that source value, while HTML uses the
canonical path. The archive table's `View` link uses the same path boundary.

Post-number permalinks, digit quote controls, comment quote links and omitted-post
`View thread` links keep their existing targets. Semantic links add no action
class or Quick Reply behavior. Thread and post IDs remain exact decimal strings
in renderer snapshots and client validation, including values above JavaScript's
safe-integer range through `9223372036854775807`.

## Client validation

The shared snapshot parser and main-thread revalidation accept an ordinary
semantic OP link only as a raw root-relative path with the matching board and
exact thread ID. The context has the same 49-byte word grammar as the server.
Encoded or normalized aliases, extra segments, queries, fragments, absolute URLs,
wrong identities and control whitespace do not become valid OP link recipes.
The existing tag, attribute, node and byte limits still apply. Depager, updater,
quote-preview and global-search consumers use this shared validation.

## Verification and limits

The HTTP/database regressions compare source JSON with board/thread HTML and
watch fragments, including saved filters, dice and fortunes after board flags
change, empty contexts, control-whitespace fallback and unchanged stored posts.
They check canonical and semantic responses, headers, quote queries, private or
missing targets, archived links and IDs above JavaScript's safe-integer range.
The JSON cases cover both listeners and thread, index, catalog and reduced-tail
projections. Pure tests also cover entity spelling, whole-word boundaries,
Unicode URL bytes, malformed paths and worker/main-thread link validation.

For this OP-link change, local checks passed 120 public library tests with one
known Unix-socket test excluded, five targeted HTTP integration cases and 165
pure-Node watcher/client cases. Generated-bundle checks and strict all-target workspace Clippy also passed.
These results do not represent a complete workspace test run.
The added Playwright cases exercise ordinary Reply navigation with JavaScript
on and off, depager insertion, search results and archived View thread links.
A real browser was unavailable locally; these browser assertions await CI and
are not recorded as passed.

The supplied files contain no original webserver rewrite configuration. Arbitrary
suffix acceptance and unusual control-whitespace routing remain unverified.
This bounded alias and OP-link change do not complete issue #216 or establish
full original-page appearance and client behavior.
