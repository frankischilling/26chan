# Native quote previews

Board and thread pages offer the **Quote preview** setting, enabled by default.
A mouseover highlights a quoted post that is already fully visible. An offscreen
or hidden local post is shown in a popup; a missing local target is fetched from
the public listener. Mouseout removes the highlight or popup and cancels pending
work. Ordinary desktop clicks and keyboard navigation keep their existing quote
destinations. No JavaScript is required to follow a quote.

## Permitted reference

The reference is the released public extension v1191, recorded in
[public-quote-preview-reference.json](public-quote-preview-reference.json).
The existing pin is 182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its configuration, `QuotePreview`, mobile quote parsing and event handlers were
read as text. No upstream JavaScript was executed and no leaked source was used.

The source enables `quotePreview` by default, including mobile devices. Its
setting label is `Quote preview`; the tip is `Show post when mousing over post
links`. It uses delegated mouseover/mouseout, without focus-triggered previews.
A local post is fully visible when its top coordinate is greater than zero and its bottom
is strictly inside the viewport, provided it is not hidden. Existing highlighted
replies receive the complementary highlight while hovered.

The source identifies mobile devices with
`/Mobile|Android|Dolfin|Opera Mobi|PlayStation Vita|Nintendo DS/` against the user
agent. This differs from the responsive layout breakpoint. Enabled mobile
quotes receive an adjacent ` #` navigation link; tapping the original quote shows
the preview, while the companion preserves navigation. The companion is removed
when previews are disabled. Its addition is preflighted against the existing
filter text and HTML budgets; a quote that cannot fit retains ordinary navigation.

Desktop popups are placed five pixels beside the quote, selecting the left side
near the viewport's right edge, and vertically centered. Mobile popups start
below the quote. The rewrite clamps both coordinates and popup dimensions to the
viewport. Existing theme variables supply the post and highlight colors.

## Projection and authority

The rewrite renders typed quote destinations as `/{board}/post/{id}`, which
already resolve to canonical thread anchors. Previews therefore use
`GET /_watch/{board}/post/{id}` instead of guessing a thread number from a reply
number or redirecting a preview request. Canonical same-origin thread anchors
can also identify a target. Board names and positive i64 identifiers are checked
before any request; requested thread membership, when known, must agree.

The new endpoint returns exactly one post:

```json
{
  "version": 1,
  "board": "demo",
  "thread": "1000001",
  "post": {"no": "1000002", "file_deleted": false, "html": "..."}
}
```

The `board_public` login reads the board, visible thread, post and approved
attachment in one read-only repeatable-read transaction. The connection is
released before rendering. The existing Askama post fragment preserves saved
comment-format stamps and media visibility. Rendering and final JSON each have
a 262,144-byte ceiling; exceeding either returns an error without a partial
projection. The endpoint accepts no query parameters or writes and is absent
from the API-only listener. ETags describe the complete representation;
Last-Modified is omitted. Visibility is checked before conditional responses,
including deleted posts, deleted threads and expired archives.

The client requests only this same-origin route, omits credentials, rejects
redirects and unexpected response URLs, requires JSON and strict UTF-8, and
counts streamed bytes before parsing. There is one request/parser at a time,
a 300 ms request interval, a five-second request deadline and a one-second parser
deadline. It uses no positive or negative cache. A later hover can observe a
deletion or recover from an earlier failure. A 404 temporarily marks the hovered
link; errors preserve its navigation destination.

Response HTML is parsed by the existing parse5 worker into a finite tree, with
16,384 nodes, depth 32 and the same byte ceiling. Exact version, field names,
board, post, thread and node grammar are validated again before DOM construction.
The live document never parses the response HTML. Local previews similarly read
a bounded recipe from the existing post without copying form values or unchecked
resource attributes. Preview construction removes duplicate IDs, forms, menus
and controls; approved media URLs retain the existing media-origin restrictions.

Settings changes, page exit, restoration from the back/forward cache and native
updater additions share the existing page lifecycle. URL linkification applies
to a preview through the existing linker. Catalog pages do not mount this feature.

The updater waits for the final page-filter result before selecting notification
priority and emitting its completion event. An insertion observer or linkifier
can replace an in-flight filter pass; cancellation of that older pass does not
establish the final result. The settlement wait follows replacement passes, with
a 60-second total deadline, at most 64 generations and updater cancellation. A
settings value that becomes readable before its storage event schedules another
pass. Failure to settle produces the existing updater error rather than a false
completion; the independent page filter can still finish normally.

## Verification and remaining scope

`apps/public/tests/quote_preview.rs` exercises the real restricted database login,
strict routing, shared rendered markup, HTTP validators, deletion and archive
visibility. Its forced table-lock interleaving checks that a concurrent commit
cannot mix board, thread and post versions. Snapshot unit tests cover exact large
IDs, saved formats, approved media, inconsistent data and byte ceilings.

The concurrent-commit check was also run with only this new reader temporarily
changed to `READ COMMITTED`. It failed at the mixed thread-state assertion.
Restoring `REPEATABLE READ` made both endpoint integration tests pass again; the
weaker isolation is not part of the change.

`native-updater-notifications.spec.js` adds a persisted regression that records
filter state inside the real `4chanThreadUpdated` event. A mixed-case URL reply
failed with linkification enabled on the earlier binary: the generated link was
present, the highlight was absent and the filter notice still said `Applying
filters...`. The disabled-linker control passed. The settlement tests additionally
hold matcher replies across observer replacement, settings changes before their
events, cancellation and deadlines, then verify a healthy retry. Both stale-input
cases failed against the earlier release bundle before the replacement guard was
added. These controlled DOM cases are separate from the persisted server test.

The preview core and persisted browser suites cover the transport and DOM
boundary separately. Persisted positive cases use the unmodified public server;
hostile response substitutions and constructed DOM inputs are identified as
adversarial fixtures. Run the targeted browser suite through the normal owned
test server with `npm run test:quote-preview`, and use `scripts/verify.sh` for the
required integration checks. Current-head CI and independent review are required
before merging the implementing PR.

The Windows preview suite has 32 cases: 18 use unmodified server responses, four
augment the DOM, four hold genuine responses to test cancellation, and six
substitute hostile responses. The integrated browser run also includes the
existing linker, page-filter, updater, notification and settings suites. The
final preview/settlement core run passed 23 tests. Each implementing PR records
the completed commands and CI results on its tested commit; local checks do not
substitute for Linux integration and CI on the final commit.

The single-post same-origin projection, finite worker grammar, bounded companion
decoration, fresh requests and stripped interactive preview controls are deliberate
security and resource choices. This implements the preview slice in #167; full
native settings-category layout, inline quote insertion and complete original-page
comparisons remain in #6. Server URL normalization and unknown raw catalog-quote
rules remain in #165. This feature does not qualify production media deployment.
