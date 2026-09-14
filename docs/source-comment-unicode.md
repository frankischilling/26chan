# Source comment Unicode cleanup

Issue: https://github.com/frankischilling/26chan/issues/131

New comments follow the supplied `imgboard.php:5295-5341, 5387-5388` Unicode
stages around the [spacing cleanup](source-comment-spacing.md). The board's raw
character budget still applies first, after CRLF/CR normalization. Both the
public handler and the board-locked text/approved-attachment insertion path use
the same preparation. Historical comments are never rewritten on reads.

On boards other than `/a/`, `/jp/`, and those configured for SJIS spacing, the
source removes U+2600..U+26FF, applies its finite ASCII-lookalike mapping, then
removes its zero-width set. NBSP and soft hyphen are removed on every board.
The fixed emoticon exclusions run next on every board; only U+2502..U+257F
box drawing is exempt when SJIS spacing is enabled. Code spacing alone creates
no exemption. Text made only of ASCII/ideographic spaces, tabs and literal pipes
becomes empty, matching the source's character class.

The mapping is the 879 active cases in `lib/postfilter.php:2611-3680`, not Unicode
normalization or general transliteration. It retains the source's omissions and
unusual outputs: fullwidth Z and two double-struck/fraktur Z characters become
`a`; small-cap i becomes `J`; script L becomes `M`; script M becomes `N`.
Commented-out mappings remain inactive. Circled w and the heavy ballot X become
`w`/`x` before the later emoticon filter can remove their original characters.
On exempt boards they are instead removed. Unlisted characters remain literal.

`strip_emoticons` at lines 104-112 is a fixed list, not a current Unicode emoji
property. U+2312 and box-drawing U+2500/U+2501 survive. After spacing and the
source's ASCII trim, `strip_private_unicode` at lines 95-102 removes every
codepoint above U+3134F. This is broader than private-use blocks. U+3134F survives;
U+31350 does not. Removal can expose edge spaces, which must not be trimmed a
second time. Ordinary long-blank-run collapse follows this removal. The complete
cleanup is therefore not assumed to be idempotent.

## Bounds and authority

Removed characters still count toward the raw board limit and independent
64,000-byte input bound. Retained or expanded output must fit the existing
16,000-scalar/64,000-byte storage bound. Emoji-only or filtered-empty fileless
comments reject before counters, clocks or post IDs change. An approved image
may carry a cleaned-empty comment, but approval, real-time expiry, single use
and the deferred attachment association remain mandatory. Invalid raw input
does not consume an otherwise valid receipt.

The Rust boundary rejects unsupported controls and malformed UTF-8 instead of
reproducing PHP's ambiguous repair behavior. HTML, including angle brackets
produced by the mapping, remains text and goes through the typed formatter and
Askama escaping. No HTML trust bypass, dependencies, runtime grants, schema
migration, visual baselines or production media enablement are added.

Deploy after migration 0027, which supplies the existing spacing policy columns.
A binary rollback affects future posts only; it cannot reconstruct characters
removed from previously stored comments. Backup and restore preserve stored
text without reprocessing.

## Verification and remaining work

An exhaustive scalar test compares the mapping and both emoticon modes against
source-derived fixtures containing all active cases and all 58 original base
exclusion ranges, including duplicates. Focused cases cover source oddities,
exceptions, filter ordering, the private ceiling, raw budgets and empty output.
Two 128-case property suites cover bounded spacing and arbitrary Unicode input.
The latter does not assume idempotence of the whole pipeline.

The database/HTTP suite checks all four operator spacing modes through both
aliases and form encodings, exact stored text and escaped JSON/HTML, historical
retention, denied policy writes and actual board-lock waits. Fileless filtered
text and shortened over-limit submissions must leave clocks/counters unchanged.
Approved-attachment fixtures cover mapped, SJIS and filtered-empty captions,
raw-limit rejection without receipt consumption, and reuse denial. HTTP image
posting includes an emoji-only caption. Four-byte limit tests use retained
U+20BB7; the no-JavaScript browser test also rejects emoji-only and raw-over-limit
comments without changing JSON or its ETag.

Local compilation/unit results do not establish database or browser posting
results. Complete current-head Linux CI is required before merge. Production
containment, recovery and independent review remain launch prerequisites.

Name/subject sanitation, spoiler/code/SJIS markup, same-board quote rewriting,
word filters, repeated-line rejection and `MAX_LINES` remain unfinished. This
slice establishes the implemented Unicode stages, not complete posting parity.
