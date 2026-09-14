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
Those samples did not establish a universal whitespace rule; the supplied
source distinguishes generated breaks from literal spaces.

The implementation projects the bounded comment token stream through the
[source catalog rules](source-catalog-teasers.md). Literal HTML remains
searchable as escaped text. Runs of generated breaks become a space, or LF for
text-only boards; other whitespace is retained. Spoilers, SJIS replacement and
the `/b/` truncation helper follow their separate source branches. Subjects are
bounded to 400 scalars independently of the 100-byte raw posting limit and its
tab expansion. Comments retain the 16,000-scalar parser limit.

## Deliberate limits still under investigation

Source entity spelling and the teaser transformation are implemented for the
current formatter. Ambient mbstring encoding, original-link presentation and
quote resolution remain unresolved or unfinished. Link attributes can affect
short `/b/` search text and the pre-strip truncation cutoff. Whole-post and
whole-catalog parity remain open.

Missing filenames are not coerced to the string `undefined`, and deleted filenames
remain excluded from both metadata and search. Filename serialization beyond the
existing public representation is not newly claimed as reference-aligned.

Issue #82 therefore remains open for these broader requirements. This change
establishes shared serialization and corrects the observed field-composition
differences; it does not redefine the full compatibility or production goal.
