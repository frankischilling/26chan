# Live catalog search

Search now updates the catalog after 250 ms of input inactivity without a
document request. Apply runs it immediately; Escape clears it; Reset clears
search and display preferences in place. Composition waits until the input is
committed. The ordinary GET form remains available without JavaScript or when
the enhancement lacks complete metadata.

The pinned v1025 client establishes the debounce, operator rules, session keys
and `#s=` restoration behavior. The local always-visible controls, input event
handling, explicit query precedence and canonical URL updates are extensions,
not evidence of full control-layout parity. Generated teaser preprocessing,
filter menus and hidden/pinned-thread behavior remain unfinished.

## Complete public snapshot

The database-backed renderer partitions one coherent board snapshot into visible
and nonmatching cards. The latter remain in `template#catalogFiltered`, not in the
rendered catalog. Clearing or widening a query can therefore show cards excluded
by the initial GET without an additional fetch. Both partitions still exclude
deleted or otherwise unavailable posts through the existing store query.

Each card carries escaped search fields and the existing numeric sort metadata.
The client moves actual nodes, not HTML strings. Inert images do not load until
their card is shown. This retains public content, not extra private authority:
all retained cards were already available through the unfiltered public catalog.
It does mean a filtered HTML response contains the complete public snapshot;
filtering is not a confidentiality boundary.

Both server and client use the bounded operator/case contract in
[catalog search](catalog-search.md). They match one serialized subject/teaser
value and test the non-deleted filename separately, following the observed field
composition. Complete teaser preprocessing and filename parity remain open under
#82. Sorting continues to use the same snapshot and preserves optional reply IDs,
sticky priority and exact integer ranks.

## State, URLs and failure handling

Search uses only `sessionStorage` keys `4chan-catalog-search` and
`4chan-catalog-search-board`; it is not written to persistent display preferences.
A fresh independent tab starts without a search. A bare catalog visit restores
a valid same-board search and clears a different board's saved search. Other
fragments suppress session restoration. A bounded `#s=` fragment accepts URL
decoding and plus-as-space. Malformed or excessive fragments are ignored safely.

An explicit `q` query takes precedence, including an explicitly empty value.
This precedence is a local URL extension. Live changes update query parameters
with `history.replaceState`; an active search fragment is kept consistent.
Unavailable history does not disable filtering. Unavailable, malformed or
oversized storage does not disable browsing or create a reload loop.

Search accepts 128 Unicode scalar values, without control characters, matching
the server limit. The input no longer uses a UTF-16 `maxlength` that would reject
valid supplementary characters prematurely. The server still bounds the raw
query, and JavaScript validates before constructing the restricted regex.
All new rendering uses existing escaped nodes, `textContent` or fixed text; no
HTML parser, broader CSP source, network fetch or new endpoint is introduced.

## Evidence

- The persisted browser workflow widens an initially filtered catalog, checks
  restored card identities and compares sage/deletion ordering without reloads.
- HTTP tests check both the visible result IDs and the complete inert partition.
- Controlled-clock browser tests check 249/250 ms boundaries, superseding input,
  composition and Escape. These use the installed [Playwright clock API](https://playwright.dev/docs/clock).
- The actual release script executes all 48 shared search cases through live
  controls, not a substitute matcher.
- Browser tests cover same-board restoration, independent tabs, board changes,
  non-search and malformed fragments, query precedence, storage failures and
  supplementary-character bounds.
- An actual-CSP image control proves the fixed image route works, then verifies
  zero hidden-image requests before activation and retained node identity after
  showing it. No screenshot baselines are refreshed for this feature.

Run public Rust tests, `npm run test:behavior` and the standard visual suites.
There are no migrations, dependency changes, staff changes or production-media
policy changes. Full compatibility and production qualification remain unproven.

## Empty catalog transitions

The live search regression covers initially filtered and unfiltered empty catalogs.
Apply, Escape, Reset, and both server-rendered and newly created Show all threads
links must restore the unfiltered empty message without navigation. A subsequent
nonempty query must restore the no-match message. These assertions prevent stale
initial HTML from determining the message after the query changes.
