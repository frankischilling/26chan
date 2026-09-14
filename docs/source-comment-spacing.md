# Source comment spacing

Issue: https://github.com/frankischilling/26chan/issues/129

New public comments receive the supplied source's whitespace cleanup after CRLF
and bare CR become LF and the pre-cleanup character limit passes. Ordinary and
approved-attachment insertion use the same preparation under the board mutation
lock. The HTTP path checks the currently visible policy before password hashing;
the locked check determines the text that is stored.

The source evidence is `imgboard.php:5295-5318, 5387, 5443, 7289-7310` and
`lib/postfilter.php:72-85`:

- Remove NBSP and soft hyphen on every board. Apply the source's fixed zero-width
  filter except on `/a/`, `/jp/`, or a board using SJIS spacing. Code spacing
  alone does not exempt that earlier filter.
- Convert ideographic spaces to ASCII spaces except on `/a/`, `/b/`, `/jp/`,
  or SJIS-spacing boards.
- Ordinary spacing collapses runs of ASCII spaces, tabs, U+200B and U+2029 to
  one ASCII space. On boards where the earlier filter removes U+200B/U+2029,
  removal happens first. Code/SJIS spacing instead expands tabs to four spaces
  and preserves other spacing.
- Trim the source's default ASCII edge-whitespace set. Ordinary spacing then
  collapses runs of four or more LF separators, with only ASCII/ideographic
  spaces between them, to one LF. Runs of up to three remain. Code/SJIS spacing
  bypasses this blank-line collapse.

Historical posts are not rewritten or normalized during reads. HTML stays raw
text in storage and is escaped by the existing typed formatter and templates;
the source's early `htmlspecialchars` call is not duplicated into stored text.
An image comment that cleans to empty still requires the approved, single-use
attachment transaction and deferred association constraint. Fileless blank
comments remain rejected. Ordinary OP subject/comment requirements are a
separate known admission gap.

## Policy and bounds

Migration 0027 adds operator-only `comment_code_spacing` and
`comment_sjis_spacing`, both false by default. These select sanitation behavior;
they do not enable code/SJIS markup or advertise unsupported API flags. The
migration applies the active source overrides to existing `/g/`, `/j/`, `/test/`
(code) and `/jp/`, `/vip/` (SJIS) boards. Operators set overrides explicitly when
creating future boards. The `/test/` commented-out duplicate is not an extra
override. No runtime role gains board-write authority.

The 64,000-byte input ceiling and 16,000-scalar global storage ceiling remain.
The board character budget is checked before cleanup, so leading spaces cannot
evade it. Four-space tab expansion may exceed the board's input count; it is
allowed while the independent global storage limit holds. Oversized expanded
output rejects before content, clocks, counters or attachment claims change.
Existing unsupported-control rejection remains a security restriction, including
form-feed and vertical-tab input that the old PHP cleanup could transform or trim.

Apply 0027 before the new binary. Older binaries can run with the additive columns
retained, but do not apply the new posting cleanup. Rolling back the binary does
not reverse already stored text transformations. Backups preserve stored text
exactly; no reprocessing is part of restore.

## Verification and remaining work

Domain tests cover exact spacing, distinct board exceptions, mixed line endings,
blank-run boundaries, pre-cleanup limits, tab-expansion bounds, controls and 128
bounded property cases. Actual database tests cover all four policy combinations,
both HTTP aliases/encodings, escaped HTML, historical text retention, public
policy-write denial, and two witnessed board-lock waits applying the committed
policy. Approved-attachment tests retain capability, approval, reuse and empty-row
checks; HTTP also submits a whitespace-only image comment. The historical upgrade
exercise applies migrations 1 through 26 before 0027 and uses actual public-role
reads and denied writes.

Local compilation and domain tests do not establish database or upgrade results.
Complete current-head Linux qualification remains required. No baselines or
production media permissions change.

The [source Unicode stages](source-comment-unicode.md) apply the finite
ASCII-lookalike mapping before zero-width cleanup, emoticon exclusions before
spacing, and the codepoint ceiling after trim. These stages can remove characters
even when SJIS exempts them from the earlier zero-width filter. Board markup,
intra-word spoiler removal, name/subject cleanup, word filters, repeated-line
rejection and `MAX_LINES` admission remain unfinished. These tests do not establish
complete source equivalence for every input.

[Same-board references](source-local-quotes.md) are rewritten before spacing
for new posts, independently of code/SJIS spacing and without changing historical
stored comments.
