# Native backlinks

The **Backlinks** setting is enabled by default on board and thread pages. A
quoted post lists the source posts in the current document that replied to it. Repeated references
from the same source create one reverse link, including self-quotes and quotes
of the original post. Links retain their ordinary navigation destinations when
JavaScript or the option is disabled.

## Public reference

The permitted released native extension v1191 supplies the behavior. Its pin is
recorded in [public-backlinks-reference.json](public-backlinks-reference.json):
182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
`Config.backlinks`, `Parser.parseBacklinks`, the initial and updater parser order,
`QuotePreview.show` and the embedded CSS were inspected as text. The setting tip
is `Show who has replied to a post`.

The reference processes source post messages in parser order and appends a row
once for each source/target pair. It includes OP targets and self-quotes. A forward
quote of its containing thread's OP gains ` (OP)`. On thread pages, a missing
same-board target gains ` →`, except for an explicitly cross-board-style label;
the board index omits that arrow. Tracked ` (You)` labels precede these suffixes.

Targets come from the current document. A quote that had no local target when
its source was first processed does not acquire a backlink retroactively when
the updater later inserts that target. New source posts can reference it normally.
Generated rows and preview copies are never parsed as new sources. There is no
remote graph lookup or new request endpoint.

Desktop rows sit inside the target's post information. Layout-mobile rows sit
below the post and carry their own adjacent ` #` navigation links, even when
quote previews are disabled. Layout-mobile means a viewport at most 480 pixels
wide unless `4chan_never_show_mobile` is the exact string `true`; this is separate
from the device user-agent test used by quote previews. Ownership keeps the two
features from duplicating or deleting each other's navigation companions.

A hidden or filtered source still contributes a backlink. Hiding the target
hides its row through the existing post/thread visibility rules. Local quote
previews can include a bounded copy of known backlink rows; mobile rows remain
hidden inside the popup. Remote previews do not discover backlinks. A preview
opened from a backlink may mark the first exact reverse quote with a dotted
underline when the original public client's direct-child conditions match.

The six themes use quote colors from their pinned version-716 stylesheets, with
the extension's specific blue-family backlink override. Their URLs, hashes and
collection timestamps are in [public-theme-reference.json](public-theme-reference.json).
The local theme-family marker chooses only these fixed rules; it does not accept
arbitrary colors or styles from a post.

## Filter and DOM boundaries

Backlink annotations occur after the source's filter projection. Applying a
filter to `(OP)` or the generated arrow must not match a post merely because
backlinks are enabled. The controller owns the exact suffix text nodes and omits
only those nodes when serializing the bounded filter input. A literal `(OP)` or
arrow written by the poster remains part of the input, as do the existing tracked
label and navigation-companion semantics. Similar-looking attributes or text in
an unowned node do not confer ownership.

Registration preflights the complete annotated message before adding labels or
graph edges. It retains the existing 65,536-character HTML and 16,384-character
text ceilings, with at most 16,384 message nodes and depth 32. Graph work is capped
at 20,001 posts, 512 quote links per post, 16,384 quote links in total and 4,096
edges. A source that exceeds a registration budget keeps ordinary navigation;
it does not produce a partial set of annotations or edges.

Only direct canonical posts in the board are admitted. Quote routes must resolve
to the same board and origin, use exact positive i64 identifiers, and agree with
explicit thread membership. This replaces the public client's prefix-blind
`#p` lookup: a cross-board or foreign numeric collision cannot bind to a local
post. No authority is inferred from a quote's display text alone.

Preview rows are constructed from the controller's known identifiers after the
existing finite post tree has been validated. Original DOM subtrees are not
cloned into the popup. The added projection is limited to 128 rows, 1,024 nodes
and 32,768 characters, and must also fit the remaining validated preview budget.
An oversized additional row projection is omitted as a whole. Duplicate IDs,
interactive post controls and unchecked resource attributes remain excluded.

Repeated refreshes, native updater insertions, cross-tab settings, layout changes
and back/forward restoration reuse the same controller. Disabling backlinks
removes its own rows and suffixes without replacing the original quote anchors.
Page exit disconnects observers and clears owned preview additions.

## Fixed release module

The graph controller is built as the fixed first-party page resource
`/static/native-backlinks.v1.js`, with a 32,768-byte release ceiling. The existing
parser worker retains its separate 262,144-byte ceiling. The page module imports
no code at runtime; its build admits only the backlink source, the existing small
ID helper and filter limits. The same pinned esbuild and license notices are used.
The existing worker exposes its already validated quote-route helper for reuse;
the graph module does not import or duplicate the HTML parser.

Only this exact page-script path is added to the page's script policy. It is not
an allowed worker source and receives no additional network, media, database or
API-listener authority. The resource response itself denies scripts, connections
and workers, serves fixed UTF-8 JavaScript bytes, supports HEAD and rejects writes.
The build checks both byte ceilings, pinned dependencies, exact exports, source
allowlists, absence of runtime imports and reproducibility.

## Verification

Run `npm run test:backlinks-core` for the graph, annotation, callback and bounded
DOM cases, and `npm run test:backlinks` for the persisted browser suite. The latter
distinguishes real form-posted positives from explicitly augmented DOM and route
fixtures. Existing quote-preview, filter, tracking, updater and notification
suites remain required. Rust asset and policy tests check the fixed resource and
its lack of worker, network and API authority; browser checks use healthy allowed
and denied contexts for the same available module.

Local Windows qualification on September 15, 2026 passed all 18 core/DOM cases
and all 50 browser cases in `tests/browser/native-backlinks.spec.js`. The browser
suite contains 35 persisted cases, 14 explicitly augmented DOM cases and one
actual back/forward-cache traversal. The latter requires a persisted `pageshow`,
the original document and quote-anchor objects, and settings changes made while
the page was cached. It is separate from the core suite's synthetic lifecycle
events. All six themes passed at both layouts. Two earlier red-hover controls
failed against the navy hover rule before the stylesheet correction.

`cargo test -p board-public --lib --test ui_assets --locked` passed 63 library
tests and five asset tests. The seven persisted updater-notification cases also
passed with tracked and backlink labels composed together. These checks establish
local behavior; the complete CI checks must pass on the reviewed PR commit before
merge.

The implementing PR records the actual local commands, failures resolved,
independent review and checks on the final commit. Synthetic visual tests guard
the rewrite's layouts; they do not establish complete original-page parity.
This slice is tracked in #168. Remaining inline-quote and native-client behavior
belongs to #6; server normalization remains #165. Production launch qualification
retains the separate requirements in [readiness.md](readiness.md).
