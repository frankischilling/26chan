# Comment admission projection

This module models the ordinary comment at the duplicate check in the pinned
original `imgboard.php:5866-5882`. The store uses its digest for private duplicate
admission history and the [final OP pattern](opening-comment-rules.md). The projection never replaces displayed comments or establishes
the original database's SQL equality semantics.

`comment_admission_projection::project` accepts a typed `PreparedComment` from
the privately constructed `PostContentInput`, after the preparer’s first marker
cleanup and before markup. It does not repeat that cleanup: newly joined
markers must survive until the final pass after wrapping. It also accepts explicit
board and source markup policy, filter-off or a filter profile with fixed
outcomes, and an optional validated existing dice/fortune outcome. Generated
randomizer HTML enters before markup and filtering, including inside unclosed
BBCode. The source link, wrapping, quote and marker passes follow. Link normalization
uses the pinned byte-mode PHP regex and callback, preserving its wildcard
before `php`, ASCII whitespace boundary and capture truthiness. Numeric static
quote terms are checked lexically, including exponents too large for floating
point. The pinned `lib/util.php` maps both source board groups to `4chan.org`;
this projection does not infer a different deployment’s domain map. The output
has private bounded data and version 1; its accessor supplies comparison data,
never permission to bypass template escaping. It also preserves PHP's false
values: empty string and exactly `0`.

The caller selects `OrdinaryWithoutExif` or `IncomingOekakiWithoutExif` only
when absence is known. `project_with_exif` accepts bounded extracted text and a
reserved source media number through the explicit `ExifPresent` path. Unknown
EXIF and privileged HTML remain unsupported. [EXIF comments](exif-comments.md)
records extraction provenance, persistence, display and pending qualification.
The incoming Oekaki comparison excludes its drawing annotation. A separate
`persisted_hash` appends the source annotation from validated stored drawing
facts before hashing. The original replay JavaScript spelling is digest input
only; it never becomes rendered HTML. A resolved source post suppresses the
replay link, as in the original formatter. Four digest examples were compared
with the supplied PHP formatter. Source `$admin_highlight`
is initialized false and never changed in this posting function; staff badge
highlighting does not add that inactive wrapper.

The existing hash-pinned `extract-wordfilter-posting-reference.php` supplies
independent PHP vectors, including a new `admission_projection` fixture group.
It exercises filter-off, policy combinations, fixed randomizers and test-filter
rolls, unclosed tags, entities, links and wrapping. Existing filtered-comment
vectors remain a second compatibility check. Regeneration with PHP 8.3.6 and
PCRE 10.42 preserved all 1,480 existing vectors and added 1,032 projection vectors.
The added cases include byte-mode link boundaries, numeric exponents, long
board names and retained dice spellings. The source vectors remain independent of the Rust formatter. Transfer tests also
compare typed frozen representations with admission digests for plain text,
quotes, markup, links, word wrapping and all five filter profiles. Hashing a
frozen representation does not establish provenance by itself: the transfer
consumer must match it against existing private comparison history.

Use the recorded PCRE version when regenerating this oracle. A trial with
PCRE 10.46 changed Unicode word-class behavior in old vectors; those changes
were rejected rather than adopted as new expectations.

The source SQL lacks an explicit
binary comparison or collation override; a commented generic schema does not
establish deployed collation. Effective source equality still requires evidence.
Private history retains unknown historical projection values rather
than backfilling from the rewrite's mixed-format `posts.comment` field.
