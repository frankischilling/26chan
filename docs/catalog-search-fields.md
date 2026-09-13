# Serialized catalog search fields

GET filtering and the fixed catalog script now consume one subject/teaser search
value, plus an optional non-deleted filename. The browser receives that value as
escaped `data-search-text`; it does not parse HTML or run a second formatter.
Older or incomplete metadata retains the validated GET fallback.

The observed public client wraps a present serialized subject in bold markup and
appends a nonempty teaser after a colon and space. Without a subject it searches
the teaser alone. This means raw-subject anchors and literal angle brackets need
not match. The bounded operator grammar itself has not changed.

## Evidence and scope

The pinned `catalog.min.1025.js` SHA256 is
`ce645b150e747f9ad1682dc005daf30d7bc1a9f2cd473b4f1e55f99adf76fa9f`.
The search verifier checks its field-composition expression as text, without
executing the upstream client. Shared synthetic cases cover serialized composition
and raw owned-post fields, including escaping, anchors, boundaries, supported
spoilers, quote text, whitespace, and entity-shaped user input.

Two public thread/catalog comparisons recorded in issue #82 matched after HTML
line breaks became spaces, formatting tags were removed, and repeated whitespace
collapsed. The official API examples also use escaped comment text and the
decimal apostrophe entity. No public user posts are committed as fixtures.

The implementation uses the existing bounded, nonrecursive comment token stream
instead of stripping tags from raw user input. Literal HTML therefore remains
searchable as escaped text, not active markup or silently discarded content.
ASCII whitespace folds across lines and token boundaries. Existing link and quote
normalization is preserved. Subjects are bounded to 120 scalars independently of
the stricter 120-byte public posting boundary; raw comments use the existing
16,000-scalar parser limit. No new teaser truncation rule is inferred.

## Deliberate limits still under investigation

Unicode whitespace is preserved rather than guessed from the ASCII samples.
Exact upstream entity spelling beyond observed examples, all formatting edge
cases, original-link presentation, and any upstream teaser truncation policy are
not proved by this change. Unsupported formatting retains the existing parser's
literal behavior. Whole-post and whole-catalog formatting parity remain open.

Missing filenames are not coerced to the string `undefined`, and deleted filenames
remain excluded from both metadata and search. Filename serialization beyond the
existing public representation is not newly claimed as reference-aligned.

Issue #82 therefore remains open for these broader requirements. This change
establishes shared serialization and corrects the observed field-composition
differences; it does not redefine the full compatibility or production goal.
