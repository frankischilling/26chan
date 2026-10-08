# Comment admission projection

This domain-only prerequisite models the ordinary comment at the duplicate
check in the pinned original `imgboard.php:5866-5882`. It does not enable that
check, alter stored or displayed comments, or choose SQL equality semantics.

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

The caller must explicitly select `OrdinaryWithoutExif`. Unknown or present
EXIF, privileged HTML, and Oekaki return unsupported errors. EXIF requires
source media timestamp and extracted-field provenance. Oekaki metadata is
appended after admission, so the incoming and persisted comparison strings
need separate stages before supporting that path. Source `$admin_highlight`
is initialized false and never changed in this posting function; staff badge
highlighting does not add that inactive wrapper.

The existing hash-pinned `extract-wordfilter-posting-reference.php` supplies
independent PHP vectors, including a new `admission_projection` fixture group.
It exercises filter-off, policy combinations, fixed randomizers and test-filter
rolls, unclosed tags, entities, links and wrapping. Existing filtered-comment
vectors remain a second compatibility check. Regeneration with PHP 8.3.6 and
PCRE 10.42 preserved all 1,480 existing vectors and added 1,032 projection vectors.
The added cases include byte-mode link boundaries, numeric exponents, long
board names and retained dice spellings. Local qualification passed all 194 domain tests (132 library and 62 integration),
strict all-target/all-feature workspace Clippy, the stored dice regression and
the server-link/word-break HTTP regressions. The public library passed 128 tests
with one Unix-socket test excluded after its socket creation was denied locally.
Ten pure Node regressions passed. Three browser-backed DOM cases could not launch
Chromium; browser discovery is not a pass. Hosted qualification is pending.

Use the recorded PCRE version when regenerating this oracle. A trial with
PCRE 10.46 changed Unicode word-class behavior in old vectors; those changes
were rejected rather than adopted as new expectations.

Duplicate enforcement remains separate. The source SQL lacks an explicit
binary comparison or collation override; a commented generic schema does not
establish deployed collation. Effective source equality still requires evidence.
Future private history must retain unknown historical projection values rather
than backfilling from the rewrite's mixed-format `posts.comment` field.
