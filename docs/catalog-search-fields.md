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
spoilers, quote text, whitespace, and entity-shaped user input. The browser suite
feeds those serialized cases to the shipped catalog script and checks its actual
in-place filtering, including absent and present filename metadata.

Two public thread/catalog comparisons recorded in issue #82 matched after HTML
line breaks became spaces, formatting tags were removed, and repeated whitespace
collapsed. The official API examples also use escaped comment text and the
decimal apostrophe entity. No public user posts are committed as fixtures.
Those samples did not establish a universal whitespace rule; the supplied
source distinguishes generated breaks from literal spaces.

A bounded read-only comparison on September 27 used the public catalog HTML and
public catalog API without retaining posts as repository fixtures. The response
hashes and aggregate counts are retained in
[`public-catalog-fields-observation.txt`](public-catalog-fields-observation.txt).
On `/g/`, the
149 threads that exposed an API comment all reproduced the embedded teaser when
runs of HTML line breaks became one space, generated tags were removed and edge
whitespace was trimmed. Fourteen embedded teasers still contained repeated ASCII
whitespace, so collapsing every whitespace run would disagree with that sample.
The same comparison found exact embedded/API subject and filename representations
for all 150 `/b/` threads. Nine `/b/` teasers ended in an ellipsis and ranged from
296 to 301 serialized characters; four untruncated teasers retained tag markup.
That one snapshot supports the implemented distinction between generated breaks
and literal whitespace, but it does not establish every entity, markup or
truncation boundary.

The implementation projects the bounded comment token stream through the
[source catalog rules](source-catalog-teasers.md). Literal HTML remains
searchable as escaped text. Runs of generated breaks become a space, or LF for
text-only boards; other whitespace is retained. Spoilers, SJIS replacement and
the `/b/` truncation helper follow their separate source branches. Subjects are
bounded to 400 scalars independently of the 100-byte raw posting limit and its
tab expansion. Comments retain the 16,000-scalar parser limit.

## Deliberate limits still under investigation

Source entity spelling and teaser preparation use the saved formatter and current
board policy. [Server comment links](server-comment-links.md) reconstruct the
source static-link representation for search and `/b/` truncation. The synthetic
PHP fixture pins UTF-8 mbstring behavior, including link-dependent length checks
and unresolved numeric quote text. GET and live search consume that shared value.

Missing filenames are not coerced to the string `undefined`, and deleted
filenames stay excluded from metadata and search. This privacy adaptation is
explicit. Original attachment normalization and download behavior remain in
#202; complete surrounding catalog layout remains in #193.

The September 15 inventory confirms that shared teaser projection, text-only
catalog tables and hover previews are integrated through #157, #160 and #164.
The active completion target now follows the
[supplied source snapshot](compatibility.md#active-reference-boundary).
