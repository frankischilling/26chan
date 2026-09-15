# Optional browser URL linkification

Board and thread pages mount the URL linker through the existing native client
bundle. Settings exposes **Linkify URLs**. Existing server-generated links keep
their nodes, destinations and formatting; the linker only wraps eligible text.
Catalog pages do not mount it. No database migration or stored-format change is
part of this integration.

## Permitted reference and remaining server questions

The behavioral reference is the released public extension v1191,
`https://s.4cdn.org/js/extension.min.1191.js`, collected September 13, 2026 at
12:55:46.721 UTC: 182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
[The navigation manifest](public-watcher-navigation-reference.json) records the
same asset. Its `Linkify`, `Parser.parsePost`, configuration defaults and mobile
layout selection were inspected as text. No upstream JavaScript was executed
for this integration, and fixtures contain synthetic content.

The original brief excludes leaked source. The earlier checkpoint relied on the
supplied `4chan-old` checkout; its remote describes that repository as leaked
source. That checkpoint's inspection and differential result are not accepted
as reference qualification for this change. The browser contract above was
independently checked against the public release.

The pinned official API `Threads.md` example already contains external HTTP(S)
anchors in `com`, as well as a static catalog quote link. Neither those examples
nor the optional client linker establish a universal server auto-link policy.
The existing server formatter and all saved profiles therefore remain intact.
Exact server URL normalization, static aliases, dead-quote resolution and their
effects on catalog text remain unresolved under [#165](https://github.com/frankischilling/26chan/issues/165),
[#82](https://github.com/frankischilling/26chan/issues/82) and
[#6](https://github.com/frankischilling/26chan/issues/6). The public FAQ establishes
ordinary same-board and cross-board quote syntax, not those server algorithms.

## Browser behavior

The public client's first probe is case sensitive and excludes a capital B or
double quote immediately before a URL. Its later case-insensitive pass can link
uppercase URLs only after that probe succeeds somewhere in the message. Quoted
href values alone do not satisfy the probe. Ports end the matched hostname;
trailing punctuation and surplus closing parentheses use separate passes against
the original match. Word breaks stay in labels but leave the destination. The
encoded derefer parameter retains serialized entity spelling.

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

The public link relation is `noreferrer ugc`; this implementation adds explicit
`noopener`. Malformed destinations and lone surrogates remain text. Validation
does not replace a valid label or parameter with URL-library serialization.
Unexpected DOM nodes, depth above 32, more than 32,001 descendant nodes or more
than 192,000 charged characters leave a message unchanged. Soft-break characters
in attributes never become markup. Request, decoded-input and output limits
remain independently enforced by the redirect handler and HTTP stack.

## Verification

`npm run test:linkification-core` checks lexical boundaries, entity/soft-break
spelling, option precedence, generated parenthesis cases, finite DOM rejection,
unchanged existing anchors, idempotence and the mounted settings/update lifecycle.
DOM fixtures block external requests and execute only the local implementation.
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
API comment content stay unchanged when the preference changes. Some other
cases replace a known generated anchor in an owned
HTTP response with text to exercise client input that the current formatter
does not emit. The updater case similarly substitutes text in a real persisted
snapshot before the normal parser and append path run. These substitutions are
explicit test inputs, not evidence that the server changed its formatting.
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
`npm run test:linkification` runs the release-bundle check, core tests and all seven
persisted browser cases; the Linux verification script invokes this command.
The current-head local and hosted results are recorded in the pull request;
full original-page parity and deployed production boundaries remain separate
requirements.
