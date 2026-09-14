# Source linkification

Issue #165 tracks the distinction between server-generated internal links and
the optional browser linker. The current application still links external URLs
on the server. The new browser module is an implementation checkpoint, not yet
served or mounted. It must not be enabled before the server profiles, derefer
handler, settings and live-update integration are implemented and qualified.

## Reference

Inspected the supplied local snapshot on September 14, 2026:

| File | SHA-256 | Relevant behavior |
| --- | --- | --- |
| `4chan-old/imgboard.php` | `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445` | Lines 3839-3882 and 4025-4138 normalize internal URLs and generate static and quote links; posting invokes normalization before word wrapping. |
| `4chan-old/js/extension.js` | `05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31` | Lines 7856-7915 implement Linkify; settings default off on desktop and on in mobile layout, after stored settings load unless disableAll is set. |
| `4chan-old/derefer.php` | `aee5b67d105020743e12a2c4fb115ea6343fc69b4166b3fc979cdf29549e8506` | Entity decoding and a two-second meta-refresh page; this is not an HTTP 302. PHP execution is not qualified here. |

The browser's first probe is case sensitive and excludes a capital B or double
quote immediately before a URL. A later case-insensitive pass can link uppercase
URLs only if that probe succeeded somewhere in the message. Quoted href values
alone do not satisfy the probe. Ports terminate the matched hostname; punctuation
and surplus closing parentheses follow separate passes against the original
match. Word breaks remain in labels but leave the destination. The derefer
parameter preserves serialized entity spelling for the server's later decoding.

## Browser implementation checkpoint

`native-linkification.js` returns bounded source-matching spans, projects escaped
text to existing DOM positions and uses Range plus finite element creation.
It does not assign user text to innerHTML or parse it as HTML. Read-only innerHTML
supplies the source's global probe after node, depth and character checks.
Replacements run backwards, preserve surrounding elements and existing anchors,
and retain source soft-break conversion in text. Existing anchors are never
nested or recreated. A second invocation does not add duplicate links.

The module also implements the option decision: disableAll wins; mobile layout
overrides stored linkify=false unless `4chan_never_show_mobile` is the exact
string `true`; desktop requires linkify=true. Actual settings and layout lifecycle
integration remain unfinished.

## Security exceptions

- Links use the application's fixed `/derefer` path instead of a hard-coded
  third-party sys origin. The handler is not yet implemented. The module is not
  exposed through the release router, so this checkpoint adds no broken UI links.
- Destinations must parse as HTTP(S), without credentials, ASCII controls or
  backslashes. Malformed URLs and lone surrogates remain text. Validation does not
  replace source spelling with URL serialization.
- Links add explicit noopener to the source noreferrer/nofollow relationship.
- Unexpected DOM nodes, over 32 levels, over 32,001 descendant nodes or over
  192,000 charged characters leave the message unchanged. Literal soft-break
  characters in attributes are not rewritten into markup. These are bounded
  rendering rules, not changes to stored comments.

The future derefer handler must separately validate decoded destinations and
referrers; the browser is not an authorization or URL-validation boundary.

## Verification

`npm run test:linkification-core` passed locally on Windows with the locked
Playwright Chromium. Four core tests cover source lexical cases, entity/break
spelling, option precedence and bounded malformed inputs, including 128 generated
parenthesis cases. A fifth test executes the actual DOM implementation against
13 synthetic cases and five unchanged-on-rejection cases with all network
requests blocked. It checks labels, original text, existing node identity,
attributes, soft breaks, nesting and repeat invocation.

A local differential check executed only the inspected Linkify object in a Node
VM with a synthetic domain mapping. All 252 bounded combinations matched the
finite DOM implementation after normalizing only the derefer origin and added
noopener. Fixtures combined prefix, case, escaped entities, soft breaks and
punctuation. No original PHP, other extension code or third-party service ran.

The first DOM run failed one incorrect test expectation that a quoted lowercase
href would activate an uppercase URL elsewhere. The source probe excludes that
prefix; separate cases now verify both href-only rejection and activation by a
lowercase anchor label. The corrected full local run passed. CI includes this
suite in both Linux verification and the Windows job; hosted results remain
pending for this checkpoint.

## Remaining acceptance work

Keep #165 open and its PR draft until the complete behavior exists: source server
internal/static links, preserved historical format profiles and new insertion
stamps, scoped attachment and migration checks, HTTP/JSON/staff/catalog rendering,
safe derefer behavior, real settings and desktop/mobile lifecycle, live updater
integration, persisted browser checks and complete current-head CI. No catalog
parity, production-media qualification or deployment is claimed by these tests.
