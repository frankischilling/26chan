# Source comment markup

[Issue #143](https://github.com/frankischilling/26chan/issues/143) covers the
whole-comment formatting pipeline. The typed BBCode passes are implemented in
`board_domain::comment_markup`. Migration 0031 stamps posting-time policy.
Public pages, JSON, updater fragments, catalog search and staff previews now
consume `parse_post_comment`. Historical format-zero posts retain their old
formatter. The complete source formatting/admission pipeline remains unfinished.

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
The parser checkpoint passed 56 domain tests and strict domain Clippy. A further
test exhausts every `i16` format value: only versions 8 through 15 grant source
markup policy. Version zero is reserved for the historical formatter. With that
test included, all 57 domain tests and 41 public library tests pass locally,
as does strict domain/store/public Clippy with all targets and features.

Migration 0031 adds `content.posts.comment_format`, with zero for every existing
row. A `BEFORE INSERT` trigger stamps new rows as 8 plus the spoiler/code/SJIS
bits from the board policy while holding a shared board lock through commit.
Both normal public insertion and the approved-attachment function run that same
trigger. Policy changes and deletion do not rewrite stamps. Public/staff roles
cannot insert or update the stamp or disable the trigger. The function is
security-invoker, has a fixed search path and has no public execute grant.
The attachment-owner role gains only SELECT on the three existing, non-private
board policy columns; its existing board-lock privilege remains unchanged.

The actual database test covers all eight masks, unchanged earlier rows,
public denials, an attachment-owner insertion rolled back without publication,
and two witnessed board-lock waits. The approved-capability matrix also checks
each accepted post's stamp alongside its existing capability/reuse assertions.
The upgrade exercise applies migrations 1 through 30 before 0031 and checks
historical text/clocks/policy, all eight new stamps, bounds and public denials.
These database and upgrade checks are compiled/supplied, not verified locally;
the local PostgreSQL service is unavailable. Complete hosted execution remains
required. A local all-feature staff build also could not configure its vendored
OpenSSL because Perl was absent from PATH; no dependency was changed to bypass it.

Apply 0031 before the new binary. Retain the additive column/trigger on rollback;
older binaries ignore the stamp. Restore must preserve stamps with stored text,
not regenerate them from current board settings. The populated restore fixture
includes operator-owned historical stamps zero and 15 and compares complete post
fingerprints across the real dump/restore. That updated restore exercise also
requires hosted qualification. Historical format zero remains
unchanged in the existing visual fixtures, including the two explicitly
historical demo seed rows. New demo posts receive the disabled source policy.

Completion still requires final post-markup empty-content admission and actual
database/browser/migration qualification.
Board flags are globally false in the source. Active overrides are spoilers on
24 boards, code on `g`, `j`, `test`, and SJIS on `jp`, `vip`; existing sanitation
columns alone must not silently change historical rendering authority.

Word filters, linkification/wrapping/quote equivalence and ordinary OP
subject-or-comment admission remain separate unfinished parts of source parity.
No production-readiness or deployment claim follows from this work.

## Runtime rendering checkpoint

The shared formatter maps the stored policy to whole-comment typed tokens, then
applies bounded quote and HTTP(S) link recognition to text nodes. Disabled
markers remain literal; the legacy spoiler tokenizer cannot grant new markup
authority. Unknown format values produce only escaped text and breaks.
Greentext spans end at generated markup/link boundaries rather than crossing
them. Full source linkification, word wrapping and quote equivalence are still
unimplemented; this is not a complete formatting-parity claim.

Both templates render only literal `s`, `pre.prettyprint` and `span.sjis` tag
cases and escape every text node. Staff quote references remain non-navigating
spans under the existing staff containment policy. No safe-HTML string, dynamic
tag name, inline style, event handler or script is accepted. Public JSON omits
`com` when the rendered comment is empty, including removed empty spoilers.
Catalog server filtering and serialized browser search fields use the same
post-stamped representation. The stamp is not a public API field.

The updater accepts only the new inert tags/classes; `pre` must have precisely
the approved class. Existing URL, attribute, ID, tree, depth, byte, node and time
limits remain in force. The release bundle is regenerated from pinned sources.
Source spoiler/SJIS/code CSS is compiled into both applications. Desktop theme
overrides and the source mobile code rules follow the source stylesheet order.
No remote fonts or syntax-highlighting script are loaded. Syntax highlighting
itself remains unfinished.

New template tests cover all eight masks, multiline/crossing markers, empty
spoilers, short-code byte boundaries, links inside spoilers, quote boundaries,
legacy rows, unknown versions and escaped HTML. Updater tests retain malformed
attribute and active-content rejection; projection tests use post stamps that
disagree with current board policy. The database test now reads saved posts
through HTML, both JSON routers, catalog filtering and updater fragments after
policy changes, with additional empty-spoiler/code/SJIS cases. Those actual
database checks await hosted execution.

Six new theme cases exercise desktop and mobile production templates, source
computed styles, spoiler hover and inert hostile text. Their first run exposed
the fixture's duplicate CSS route, which omitted the new shared styles; the
fixture now uses the actual public CSS handler. No assertion was relaxed.
The existing screenshot baselines are unchanged. A staff build attempt using
Git's installed Perl reached OpenSSL configuration but failed because that Perl
lacks `Locale::Maketext::Simple`. No dependency or system installation was changed.

Local runtime checks passed all 57 domain tests, 45 public library tests, strict
domain/store/public all-target/all-feature Clippy, the complete watcher/filter
Node suite and 58 visual cases (six markup themes, 39 media, ten public states,
three base views). Mobile Yotsuba and desktop Tomorrow markup images were
inspected. The six markup cases passed again after the final CSS-handler change.

The earlier metadata head `76a6be2` completed push Linux job 103957422585 on
September 14, 2026. Its actual log confirms the policy/lock test, eight-mask
upgrade exercise and populated restore succeeded, together with the complete
job. This qualifies that metadata implementation, not the later renderer or
expanded HTTP assertions. Full exact-head checks are still required for those.

## Final comment admission

`prepare_post_content` applies the source check at `imgboard.php:5790-5802`
after sanitation and the typed SJIS/spoiler/code passes. An ordinary OP needs
a nonempty cleaned subject or rendered comment, including when an attachment
is supplied. A reply with blank rendered content needs an authorized attachment;
its subject does not replace that requirement. Empty spoilers and short empty
code blocks can disappear. Literal disabled markers, escaped user HTML and
generated code/SJIS elements count as content. The final blank check uses ASCII
whitespace and generated breaks, without applying another Unicode trim.

The HTTP handler and the insertion transaction use the same function. The
transaction checks the board policy while holding its row lock, before assigning
a post number or changing threads. Rejected attachment posts leave their approval
available for a subsequent valid reply. Public forms allow an empty OP comment;
fileless replies retain browser-required comments. Server checks still decide
whether sanitized or generated content is blank.

Migration 0032 permits an empty stored OP comment when its subject is nonempty.
The deferred attachment constraint reads the final row and also runs for subject
updates. Clearing a subject without replacement comment or attachment fails.
The attachment owner gains only a subject-column read; temporary function/schema
authority is revoked. Historical content and formatting stamps are unchanged.
Apply 0031 and 0032 before the new binary. Retain both migrations on binary
rollback: old admission may reject new subject-only submissions but existing
subject-only posts remain readable. The restore fixture includes such an OP.

Coverage includes domain admission/precedence cases, both POST routes and form
encodings, HTML/JSON responses, all eight markup masks, observed policy-lock
waits, approved-attachment rejection/reuse, a populated 0031-to-0032 migration,
final-row deferred updates, and native-form browser cases with JavaScript on and
off. The first local domain run exposed incorrect expectations for NBSP and
ideographic-only whitespace in the tests; the earlier source sanitation removes
those, so the tests now distinguish sanitation from final admission. Strict
Clippy and the corrected domain cases passed. Local migration execution failed
with `PoolTimedOut`; database, migration, restore and browser results require
fresh hosted checks.

The source's `TEXT_ONLY` branch separately requires an OP subject. That board
policy and the privileged options/image bypass are still unimplemented; neither
is inferred from production media being disabled. OP markup, word filters,
linkification, wrapping, full quote equivalence and syntax highlighting remain
open under #143/#6. This slice does not establish complete formatting parity.
