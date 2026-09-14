# Source catalog teaser preparation

The supplied `catalog.php:126-145` prepares the embedded HTML catalog teaser
from an already formatted comment. The public JSON catalog has a different
serializer. `js/catalog.js:1754-1804` uses the serialized teaser both for display
and for subject/comment search. The file hashes and source versions are pinned
in the compatibility inventory.

The Rust catalog applies that preparation to the saved comment format. Adjacent
HTML breaks become one space, or one LF on text-only boards. Other whitespace
is retained. Ordinary boards remove generated tags except spoiler `s` elements;
literal user HTML stays escaped text. With the board's SJIS policy enabled, the
first closing span ends each replacement with `[SJIS]`, including a nested span
of another kind. A literal LF prevents that source regex match, so text-only
boards retain multiline SJIS text after tags are removed.

On `/b/`, `imgboard.php:272-314` applies its 300-character helper. It replaces
SJIS before measuring the serialized comment. A short result retains generated
markup. A longer result loses tags except spoilers and is cut at 300 serialized
Unicode scalars. A partial entity or spoiler tag is discarded, open spoilers
are closed, and U+2026 is appended. The source reuses its length from before tag
removal: it still appends the ellipsis when stripping alone made the text short.
Word-break elements contribute to that earlier length and disappear when tags
are stripped. This is not a 300-character limit on every board.

The immutable format-zero demo uses the earlier `span.spoiler` representation.
Like the source's tag-stripping rule, the projection retains its text but removes
that span. Its catalog snapshot therefore shows plain text instead of the old
black spoiler background. Newly stamped `s` spoilers retain their concealment.
The demo's thread rendering remains tied to its saved format.

Catalog display and search use one prepared token list. Askama renders the
finite tokens with escaped text; serialized search metadata never grants an
HTML-safe bypass. A present subject has bold markup, with colon-space only when
the prepared teaser is nonempty. GET filtering and the release browser script
consume the same entity-spelled search value. Excluded cards remain inert and
retain that metadata for restoration. Deleted filenames stay excluded under
the existing privacy exception.

This projection uses current board policy, as the supplied catalog generator
does. A later SJIS or text-only setting change may change a catalog teaser;
it does not rewrite the stored post or its thread/JSON comment formatter.
No migration or new runtime database grant is needed. Deploy the public binary;
existing pages acquire the new metadata when reloaded. The explicit synthetic
seed includes `/b/` and `/sjis/` cases alongside the existing text-only fixture.

The implementation pins UTF-8 scalar behavior. The supplied helper depends on
ambient mbstring encoding, which the source snapshot does not establish.
Source linkification and quote resolution remain separate unfinished stages:
short `/b/` comments retain the existing safe link representation, and its
generated attributes contribute to the helper's length and search text. This
does not prove identical source link spelling or identical cutoffs around
those unresolved attributes. The rewrite does not generate abbreviation spans;
literal user text resembling them must not be removed. Full text-only catalog
layout and filename serialization remain under issue #82 and the compatibility
inventory. These limits do not prevent the known teaser transformations from
being applied to the formatter that exists today.

Unit checks cover whitespace, spoilers, SJIS span endings and LF behavior,
short/long `/b/` comments, pre-strip length, scalar and entity boundaries,
spoiler closure and bounded hostile text. Persisted browser checks use real
public posting with JavaScript disabled, GET filtering, live search and card
restoration on all three board policies. Desktop/mobile captures are reviewed
separately from the existing screenshot baselines. The Linux verification script
runs the new browser test after the full behavior suite.

The Windows visual review changed the catalog-page baseline for the prepared
teaser. The twelve desktop/mobile menu crops also change where the teaser shows
beside their edges. Pixel comparison against the prior baselines found no change
more than two pixels from the crop edges. Menu colors, fonts, bounds and pin
behavior retain their existing assertions; no menu CSS changes are included.
Four attachment-catalog baselines also reflect the same teaser transformation
in small/large desktop/mobile modes. Their reviewed diffs retain image geometry
and show text reflow; the large mobile page becomes one 15-pixel line shorter.
Board, thread, archived and teaser-off baselines are unchanged.
