# Optional browser URL linkification

Board and thread pages mount the URL linker through the existing native client
bundle. Settings exposes **Linkify URLs**. Existing server-generated links keep
their nodes, destinations and formatting; the linker only wraps eligible text.
Catalog pages do not mount it. [Server comment links](server-comment-links.md)
describe the saved source policy and its migration; the browser preference
does not change that policy.

## Browser reference

The current behavioral reference is `js/extension.js` from supplied checkout
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Its SHA-256 is
`05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31`.
The captured [Linkify object](../fixtures/source-linkification.json) includes
the source desktop/mobile defaults. The extraction script checks both the
complete file and the isolated object's hashes. CI can check the captured
object when the original checkout is absent.

The initial implementation used released public extension v1191,
`https://s.4cdn.org/js/extension.min.1191.js`, collected September 13, 2026 at
12:55:46.721 UTC: 182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
[The navigation manifest](public-watcher-navigation-reference.json) records the
same asset. Its `Linkify`, `Parser.parsePost`, configuration defaults and mobile
layout selection were inspected as text. The supplied-source comparison now
executes only the extracted `Linkify` object on bounded synthetic strings in an
isolated JavaScript context. It runs no original startup, storage or network code.
The original serialized output is test evidence, never application rendering input.

The pinned official API `Threads.md` example already contains external HTTP(S)
anchors in `com`, as well as a static catalog quote link. Neither those examples
nor the optional client linker establish a universal server auto-link policy.
The current server policy follows the supplied source functions. It preserves
historical formatter profiles and leaves external URLs as text on new posts.
The broader source target, including remaining quote-resolution behavior, is
tracked in [#191](https://github.com/frankischilling/26chan/issues/191) and
[#208](https://github.com/frankischilling/26chan/issues/208).

## Browser behavior

The public client's first probe is case sensitive and excludes a capital B or
double quote immediately before a URL. Its later case-insensitive pass can link
uppercase URLs only after that probe succeeds somewhere in the message. Quoted
href values alone do not satisfy the probe. Ports end the matched hostname;
trailing punctuation and surplus closing parentheses use separate passes against
the original match. Word breaks stay in labels but leave the destination. The
encoded derefer parameter retains serialized entity spelling. A nonbreaking
space in an admitted DOM serializes as `&nbsp;` and stays within the matched URL, including its
derefer parameter. The DOM range maps all six serialized characters back to
the original text character; it leaves the label and surrounding nodes intact.

The option defaults off on desktop. At widths of 480 pixels or less, the mobile
default enables it even when stored `linkify` is false, unless
`4chan_never_show_mobile` is exactly the string `true`. `disableAll` takes
precedence. Unavailable storage uses the normal layout default. These defaults
are established by the public asset, rather than inferred from screenshots.

The local integration also applies changes on settings saves, cross-tab storage
events and viewport changes. Newly inserted updater replies and eligible
`#quote-preview .postMessage` elements receive the same processing. This does not
implement a quote-preview feature or claim parity for its missing UI. Disabling
linkification unwraps only links marked as created by this module. Existing
server anchors are preserved, including their node identity. A terminal page
exit disconnects the observer and listeners; pages retained in the browser's
back/forward cache keep them for restoration.

`native-linkification.js` computes bounded spans and maps them to existing text
nodes. It creates anchors with DOM Range and finite element construction, never
by assigning user strings to `innerHTML`. A bounded read-only serialization
supplies the initial probe. Replacements run backwards, preserve surrounding
markup and soft breaks, and do not nest or recreate existing anchors. Repeated
processing does not add duplicate links.

Page filters consume the final message HTML through their existing 65,536-code-
unit parser ceiling. Before changing a live message, the linker applies its
complete plan to a bounded detached clone and measures that exact serialization,
including anchor attributes and literal zero-width-space conversion to `wbr`.
If decoration would exceed the filter ceiling, the original message stays
unchanged. This preserves filter behavior and server-anchor identity without
increasing parser limits or retaining a stale copy of the comment.

## Redirect and security boundaries

Generated links use the fixed same-origin `/derefer?url=` route rather than the
reference's separate sys origin. The Rust handler independently validates the
query and destination; browser validation is not trusted. It performs no network
request and holds no additional database or media authority.

The handler decodes the supported HTML entities once, accepts only explicit
HTTP(S) destinations with a host, and rejects credentials, ASCII controls and
backslashes. Duplicate URL parameters are rejected. A supplied referrer must
have the exact configured public origin; an absent referrer is allowed because
the link has `noreferrer`. An escaped, script-free page displays the destination
host and uses a two-second meta refresh. Responses have `Cache-Control: no-store`. This
response design is a project-defined replacement: the public client reference
establishes its target URL shape, not the original server's response semantics.

The supplied source's link relation is `noreferrer nofollow`; this implementation
adds explicit `noopener`. Malformed destinations and lone surrogates remain text. Validation
does not replace a valid label or parameter with URL-library serialization.
Unexpected DOM nodes, depth above 32, more than 32,001 descendant nodes or more
than 192,000 charged characters leave a message unchanged. Soft-break characters
in attributes never become markup. Request, decoded-input and output limits
remain independently enforced by the redirect handler and HTTP stack.

## Verification

`npm run test:linkification-core` checks lexical boundaries, entity/soft-break
spelling, option precedence, generated parenthesis cases, finite DOM rejection,
unchanged existing anchors, idempotence and the mounted settings/update lifecycle.
DOM fixtures block external requests. The source corpus compares the finite DOM
result with the extracted original object, using the browser's initial HTML
serialization on both sides. It checks labels, encoded parameters, link relations,
text and soft breaks across punctuation, entities, existing anchors and markup.
The output-budget regression passes the actual before/after HTML through the
production filter parser. Its 15,419-character comment occupies 15,452 HTML code
units before linking; the previous behavior creates 700 anchors and expands it
to 120,452, which the filter rejects. The fixed path leaves that message intact.
A separate case covers literal zero-width-space expansion, and a normal URL
control still linkifies and remains parseable.

`tests/browser/native-linkification.spec.js` uses real persisted posts and the
production page/client. Its unmodified-response case posts mixed lowercase and
uppercase URLs, preserves the lowercase server anchor, and links the uppercase
text on the initial page and through the real updater. It checks that HTML and
API comment content stay unchanged when the preference changes. The entity case
posts through the real handler and checks script-free HTML, initial rendering,
unchanged JSON, derefer admission and updater insertion. As in `imgboard.php:5318`
and the source post filter, ordinary posting removes literal nonbreaking spaces
before storage. Literal `&nbsp;` text stays escaped; its serialized `&amp;nbsp;`
spelling stays in the browser's parameter. Actual nonbreaking-space DOM input
is qualified separately by the isolated source corpus. Askama's numeric
ampersand spelling in server HTML normalizes to `&amp;` in the browser.
Source-profile JSON now emits the source ENT_QUOTES spelling directly after
safe template escaping. Historical JSON profiles keep their previous spelling.
Other checks cover existing server anchors, desktop/mobile settings, disabled or
unavailable storage, live option changes, synthetic preview insertion, bounded
DOM and absence of a catalog mount.

The filter-interaction browser case substitutes a synthetic comment within the
supported 16,000-character ceiling into an owned post response because the demo
board uses a smaller posting limit. It activates linking through the real
settings UI in another tab, observes the storage event, and requires the first
tab's HTML, server-anchor object and active filter to survive. The same test
fails against the frozen pre-fix public binary at the 700 generated anchors.
This is client-integration evidence; it does not claim the demo board accepted
an oversized post or establish a new server-formatting rule.

`cargo test -p board-public --test derefer --locked` exercises the real router,
escaping, redirects, CSP, cache policy, assets and invalid destinations/referrers
without requiring a database. Handler unit tests cover decoding and bounds.
`npm run test:linkification` runs the release-bundle check, core tests and all eight
persisted browser cases; the Linux verification script invokes this command.
The current-head local and hosted results are recorded in the pull request;
full original-page parity and deployed production boundaries remain separate
requirements.

To recheck the capture against an installed original checkout:

```bash
node scripts/record-source-linkification.mjs --check --source /path/to/4chan-old
```

Numeric quote conversion and exact JSON entity spelling have separate source
and persisted checks in [source quote resolution](source-quote-resolution.md).
Complete original-page visual comparisons remain pending. The URL-linking
checks alone do not establish full frontend parity.
