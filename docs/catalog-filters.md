# Catalog Filters & Highlights

The catalog has a separate Filters control using the public `catalog-filters`
format. It opens the rule table, color palette and help panel. Rules can be
enabled, reordered, searched and deleted; Hide and Top remain independent
checkboxes. Saving closes the editor after a successful persistent write.
Opening it again shows the saved rules and their current hit counts.

Matching follows public catalog client v1025: case-insensitive whole words,
space-separated AND terms, normalized `|` alternatives, word wildcards,
case-sensitive quoted strings, `/pattern/` or `/pattern/i`, `#` tripcodes and
`##` names. Boards are separated by literal spaces. The first matching active
rule wins. A published capcode follows the tripcode as `!#mod`, `!#admin` or its
other public label; it conveys no browser or server authority.

Hide excludes matching cards and updates the top/bottom Filtered counts. Top
moves matching cards before ordinary cards while preserving sticky priority.
Color adds the released thumbnail highlight. The public image-catalog client
constructs malformed teaser markup when a color is present, leaving the teaser
text uncolored. The rewrite preserves that visible result through safe DOM
operations. Active Search, the Hidden view and manually pinned threads bypass
catalog rules. Manually hidden threads are excluded before hit counting.

## Independent reference

[The replay](../scripts/verify-public-catalog-filters-reference.mjs) executes the
whole unchanged, hash-pinned public catalog client v1025 with three owned cards.
[The record](public-catalog-filters-reference.json) contains 34 matching and
integration vectors, 28 editor states at desktop/mobile widths and 24 style
cases across six themes, two widths and densities 1/2. Browser requests outside
the pinned assets and owned fixture are denied. Page errors are checked after
each recorded interaction. The clock and Chromium version are fixed.

The eight unchanged question icons have exact public URLs, collection times,
dimensions, byte lengths and SHA-256 hashes in
[their manifest](public-catalog-filter-assets.json). The local asset table adds
eight literal GET/HEAD routes and the CSP names each path. It admits no upstream
origin, arbitrary directory or runtime asset download.

These comparisons establish the listed rule, editor and style properties. They
do not establish complete original-page pixels, original server teaser/entity
normalization in #82, or every unrecorded catalog setting. No production posts,
filenames, media or private source fixtures are used.

## Resource and storage boundaries

Patterns execute only in a fresh disposable worker. Each job allows at most 64
rules, 512 cards, 1,024-character patterns/board lists, 65,536-character card
fields, a 2,097,152-character serialized request and a 65,536-character response.
Thread IDs remain
canonical decimal strings through the full positive i64 range. Response IDs
must belong to the request in its original order and refer to eligible rules.
A 1,000 ms external deadline terminates the worker. The complete catalog cycle
is separately limited to 60 seconds. Cancellation, invalid data, an unavailable
worker or a timeout clears rule effects and leaves threads visible; no pattern
runs on the UI thread as a fallback.

Only catalog pages admit the two fixed modules and the new worker path. The
worker response denies network requests, imports and nested workers. Stored
patterns never become HTML, callbacks or URLs. Colors use the existing finite
native-filter validator. The editor uses native dialogs and text nodes; the
help panel fits within the mobile viewport so its controls remain reachable.
This bounded replacement differs from the released floating HTML panels.

Storage accepts a bounded object of numeric rule keys, with numeric or boolean
flags; saving emits numeric flags and contiguous numeric keys. At most 131,072
characters of stored rule JSON are admitted. The existing hidden-thread reader now accepts
the public numeric `1` as well as its local boolean form. Local hidden-state
writes retain their existing canonical boolean representation.

Saves require the shared Web Lock and compare the exact raw value captured when
the editor opened. A conflicting tab leaves its newer value intact and requires
reopening the editor. Closing, removing the owning form/container, `pagehide`
and BFCache suspend cancel queued work. If storage or Web Locks are unavailable,
edits stay in the current tab and the editor states that limitation. There is
no unlocked persistent write.

[Settings transfer](native-settings-transfer.md) still rejects incoming
`catalogFilters` and omits them from exports. The new local consumer does not
by itself qualify restore review, pattern validation and multi-key rollback for
this additional format. That integration remains unfinished.

## Verification

The independent replay passes 34 matching vectors, 28 editor states and 24 style
cases. All 35 core cases pass, including a real worker-thread pathological
pattern terminated at its deadline followed by a healthy fresh worker. The
combined Windows production-template run passes all 110 Search, filter and
filter-lifecycle cases. The 89 public library tests and nine fixed-asset tests
pass with zero ignored cases. Public all-target/all-feature Clippy passes.
The production-server catalog, posting, preference and CSP run passed 44 of 47
cases before the three test setup corrections described below. The focused
rerun passes all four worker/image/script CSP cases, including healthy controls
for every denied capability and zero page errors.

The full Windows theme run passed 513 of 514 cases in 7.3 minutes. Its remaining
text-catalog case caught an empty initial filter refresh rendering pinned rows
twice and consuming their reply delta. An empty refresh now leaves the original
render alone. All 178 catalog/text-catalog theme cases pass after that fix in
2.6 minutes, including the failing delta assertion. The other full-suite cases
had passed before the correction. All five generated bundle checks pass.

The owned Linux/PostgreSQL qualification passes all 93 public library tests,
nine fixed-asset tests, public all-target/all-feature Clippy, 35 filter-core cases
and all 47 production-server catalog/posting/CSP cases. There are zero ignored
Rust/core cases. The first browser startup overlapped a Windows fixture listener
and refused to reuse it; the separate rerun used the Linux server. The ignored
qualification launcher initially had Windows line endings, corrected before
it ran any checks. These local results qualify the stated scope; hosted checks
remain required before merging.

Earlier runs exposed real integration mistakes: teaser text was colored despite
the public markup quirk; mobile field fonts/margins and the blue panel border
differed; an empty filter refresh unnecessarily sorted Search's initial cards;
and the help icon inherited a margin absent from the released catalog CSS.
Each was corrected against the unchanged reference. A negative storage fixture
initially expected a fresh sort instead of its original server order; its
assertion now checks that original order and all three visible cards.

The first Rust asset run passed eight of nine tests because its exact CSP
manifest omitted the eight new icon entries. Adding that manifest made all nine
pass without widening the policy. The first production browser run also found
a missing dimension mapping, a probe listener installed after the normal worker
listener and an existing exact script-list assertion missing the two new module
paths. Those test setup corrections retain the denial controls and real response
headers. No screenshot baseline, pixel tolerance, retry or timeout is changed.

No migration, dependency, staff permission, media-processing authority or
credential policy changes in this slice. Hosted checks remain required before
the next integration merges.
