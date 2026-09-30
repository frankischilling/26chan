# Mobile post headers

The released [v1191 client](https://s.4cdn.org/js/extension.min.1191.js)
builds a separate `postInfoM` header before the OP file. Its first child holds
the name, public identity and OP subject; its second holds the date and the
two post-number links. The desktop header remains in the same post. Mobile
menus precede the identity block, and local-time formatting replaces the
date's first text node while retaining both links.

## Reference scope

`public-mobile-header-reference.json` records 288 synthetic cases: six desktop
styles, both mobile stylesheet families, ordinary and dark classes, six staff
roles, and OP/reply posts. The facts cover header order, floats, fonts, colors,
padding, borders, fixed icon sizes and both number-link hover colors. The
reference script checks the released client, eight stylesheet hashes and all
nine fixed icon hashes before constructing any page. It executes the inspected
formatter with fixed inputs and reads the extension's static CSS literal.
Extension initialization and external requests are excluded from that run.

These properties describe the header and its containing post. They do not
qualify original full-page pixels, attachment layout, the released menu or
tooltip UI, or original-server input rules. The production template is checked
against the recorded properties in `tests/themes/mobile-post-headers.spec.js`.
Preference and menu transitions use separate persisted-page tests.
The mobile post rules apply only to board and thread pages; catalog pages keep
their separately referenced catalog styles.

## Short labels

The formatter tests serialized UTF-16 length against 30 before shortening.
Its pure helper replaces the first comma entity, decodes five entities in a
fixed order, takes 30 UTF-16 units, escapes the result, and appends `(...)`.
An entity-expanded label can therefore receive the suffix even when its
decoded text contains fewer than 30 units. The cutoff can split a surrogate
pair or a combining sequence.

`public-mobile-label-reference.json` retains ten synthetic helper vectors and
a serialized comma control. Split surrogates are recorded as exact numeric
UTF-16 units; the visible result uses the replacement character so the Rust
template can emit valid UTF-8. Both the Rust view and browser recipe validator
are tested against those vectors.

This renderer explicitly HTML-escapes its stored name and subject before
applying that condition. Its public JSON name remains raw text. The vectors
do not establish how the original server serializes names or which inputs it
accepts. Shortened native labels use an escaped full-text `title`; the released
client's custom tooltip callback is not reproduced by that attribute.

## Ownership and bounds

The finite post recipe binds the mobile identity, shortened labels, full-label
titles, timestamp and both link targets to the same post's desktop header.
Mixed reply targets, extra headers, foreign IDs, body forgeries, arbitrary
titles and incomplete staff badges fail validation. Local and remote quote
copies retain the inert presentation and lose IDs, forms and original-post
actions. ID tooltips count distinct original posts across the two headers.

The 262,144-byte parser asset ceiling, 32,768-byte Quick Reply ceiling,
node/depth limits and request deadlines remain enforced. The native menu moves
one existing trigger between the original headers when the viewport or exact
`4chan_never_show_mobile` value changes. Mobile dark mode keeps the selected
desktop stylesheet and applies the released dark class; desktop dark mode uses
the existing local stylesheet override.

```sh
node scripts/verify-public-mobile-label-reference.mjs /path/to/extension.1191.js
node scripts/verify-public-mobile-header-reference.mjs /path/to/extension.1191.js /path/to/desktop-css /path/to/mobile-css
node --test tests/browser/native-mobile-label.test.mjs tests/browser/native-updater.test.mjs
npx playwright test tests/browser/mobile-post-headers.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/mobile-post-headers.spec.js
```
