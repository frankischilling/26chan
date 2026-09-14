# Source comment markup

[Issue #143](https://github.com/frankischilling/26chan/issues/143) covers the
whole-comment formatting pipeline. The typed BBCode passes are implemented in
`board_domain::comment_markup`; posting, persisted policy and renderers are not
yet connected to them. Existing public and staff formatting remains in use.
Parser tests do not establish runtime formatting parity.

## Source passes

The supplied `imgboard.php:497-586, 5751-5790` provides these rules:

- Turn prepared LF separators into linebreak nodes, then apply SJIS, spoiler and
  code passes in that order. User-supplied HTML remains text.
- Match exact lowercase markers. Preserve unmatched closing markers before the
  first opening marker. After parsing starts, discard unmatched closing markers.
  Track all source nesting but emit only two levels for spoilers/code and one
  for SJIS. Close remaining emitted levels at the end of the comment.
- SJIS selects its spoiler check when any literal `[spoiler]` exists. A spoiler
  opening or closing marker in a segment preceding an SJIS closing marker, or
  in its still-open final tail, cancels the entire SJIS pass. When that check is
  selected and the segment has no spoiler marker, the source omits text before
  SJIS closing markers. Tests retain this source quirk, including spoilers
  outside the SJIS block.
- Remove spoilers containing only ASCII whitespace, linebreaks or recursively
  empty spoilers. Literal HTML-looking text, NBSP and ideographic spaces do not
  count as those generated elements or ASCII whitespace.
- Before code parsing, unwrap matched code pairs whose body occupies at most
  six bytes in the source's escaped/generated HTML representation. This counts
  UTF-8 bytes, the five source HTML entities and generated element lengths,
  not Unicode characters or visible text. Remove one initial linebreak after
  each generated code opening, then reduce four or more consecutive linebreaks
  to three throughout code-enabled comments.

The output contains text, linebreaks and a finite enum of opening/closing tags.
It preserves crossing tag kinds from the ordered source passes. It is not a
recursive DOM tree. Consumers must escape text and render only literal approved
elements for the enum cases; no arbitrary HTML or template-safe string is part
of the interface. Linkification, word wrapping and quote formatting belong to
later stages, and do not run inside this parser.

Input is prepared text with LF separators. Independent callers are capped at
16,000 Unicode scalar values. The implementation makes a fixed number of linear
passes, with constant-width marker comparisons and no recursion. The source
nesting counter is bounded by input size; emitted nesting is bounded per kind.

## Verification and integration work

Seven integration tests cover multiline and malformed markers, nesting limits,
empty spoilers, source byte boundaries, SJIS rollback/omission, ordered crossing
tags, code linebreaks, disabled flags and maximum multibyte input. One of those
tests generates 128 bounded mixed-marker/Unicode cases and checks output bounds
and per-kind balancing. A separate 128-case unit test compares each generic
typed pass with an independent string/byte-offset transcription of the source.
The complete domain suite passes 55 tests; strict domain Clippy also passes.

No database schema, grants, stored posts, posting decisions, HTML, JSON,
updater output, catalog text or staff preview changes are included yet.
Completion still requires post-time policy and historical-format semantics,
normal and approved-attachment transaction integration, shared safe rendering,
all public/staff consumers, and actual database/browser/migration qualification.
Board flags are globally false in the source. Active overrides are spoilers on
24 boards, code on `g`, `j`, `test`, and SJIS on `jp`, `vip`; existing sanitation
columns alone must not silently change historical rendering authority.

Word filters, linkification/wrapping/quote equivalence and ordinary OP
subject-or-comment admission remain separate unfinished parts of source parity.
No production-readiness or deployment claim follows from this parser work.
