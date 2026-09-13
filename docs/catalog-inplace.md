# In-place catalog display controls

The pinned public v1025 client updates sort, image size and teaser display
without navigating the document. These three controls now operate on the local
server-rendered snapshot in place. Saved display preferences also restore
without a second document request. Search submission and Reset still use GET;
live search, filtering menus, hidden/pinned threads and complete catalog parity
remain unfinished. Explicit query URLs and their precedence are local extensions.

## Snapshot and browser boundaries

The existing repeatable-read board snapshot supplies each card's latest visible
reply ID, visible reply count, bump timestamp, thread ID and sticky flag. The
view carries the already-computed latest reply ID rather than issuing another
query or deriving it from the OP-only catalog preview. All sorts keep sticky
threads first and use the same deterministic tie ordering as the server.
Integer ranks use JavaScript `BigInt`, avoiding precision loss for PostgreSQL
identifiers above JavaScript's safe-number range. Display changes do not fetch a
new snapshot; reload is required to observe subsequent posts or deletions.

An absent latest reply stays optional through the view and becomes an empty
metadata attribute, not a fabricated OP ID. The client keeps absent replies below
present replies and uses ascending thread IDs for equal last-reply ranks, matching
the server's optional-value ordering.

The server also emits both thumbnail dimension pairs using its existing bounded,
non-upscaling calculation. No browser media decoding supplies trusted metadata.
Deleted, spoiler and no-file fallback images are not resized by this logic.

Teasers remain Askama-escaped formatting output. When initially off, their nodes
are held in an inert template. Toggling moves those same nodes into or out of the
card; it never parses a stored HTML string. Sorting retains the actual card nodes
and their inline-block whitespace. No `innerHTML`, executable stored values,
third-party script, additional API endpoint or expanded CSP source is required.

The enhancement checks all rank fields, sticky flags and available thumbnail
dimensions before it changes the catalog. Invalid metadata leaves the original
GET form as the fallback. The existing no-JavaScript renderer, query validation,
search behavior and finite browser preference parsing remain in place. These are
public presentation fields, not new authority over posts, media or staff state.

No migration, dependency or production-media policy changes are included.

## Evidence

- The persisted browser workflow tests all four sorts against real sage replies,
  then deletes a reply and compares the reloaded client snapshot with the actual
  no-JavaScript server result. It checks visible counts, retained DOM identities
  and zero document requests during display changes.
- Browser preference tests require a fresh tab to restore using one document
  navigation, while retaining storage-failure and actual CSP controls.
- Synthetic browser metadata checks exercise adjacent IDs above `2^53`, sticky
  priority, tie ordering and malformed-rank fallback through the real GET route.
  They use the release script and actual catalog CSP, not a substitute sorter.
- Six theme tests cover both pinned widths and all four size/teaser combinations.
  They compare actual image attributes and screenshot bytes between in-place and
  server-rendered modes, including restoration of initially hidden teaser nodes.
  These comparisons create no new screenshot baseline.
- Existing Rust database/snapshot tests and the full visual/state suites remain
  required. The changed view field is used by production handlers and all fixture
  constructors; fixture renders do not claim to prove database isolation.

Run the public Rust tests, `npm run test:behavior`, `npm run test:themes` and the
standard visual suites in the owned test environments. Passing these tests does
not establish unimplemented reference behavior or qualify production deployment.

The combined browser workflows share one loopback peer. Adding the reply-deletion
check took that suite past the default 30-write window; the first full run reached
the expected rate rejection during its last cleanup. The browser-only server now
uses an explicit 60-write budget. Application defaults are unchanged, and the
actual HTTP tests still assert both the default 30-write cap and configured caps.
No behavior assertion was removed to bypass the rejection.
