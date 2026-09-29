# Native Custom CSS

The pinned public extension v1191 exposes a `Custom CSS` setting that defaults off, stores the editor text under `4chan-css`, and provides an `Edit` action. When the setting is enabled, the public extension inserts the saved text into a style element. The rewrite keeps the setting, storage key, editor, and enabled/disabled behavior, but narrows what saved CSS can do.

Raw stored CSS is not inserted into the document. `native-custom-css.v1.js` parses the complete draft first and accepts it only when every selector, property, and value is in the supported post-style language. A valid draft is rebuilt from normalized tokens and placed in a constructed `CSSStyleSheet`. Every generated selector is prefixed with `.board`, and no `<style>` element or inline style attribute is created. This keeps the existing `style-src 'self'` policy; the feature does not require `unsafe-inline`, `data:`, or `blob:` style sources.

The editor remains available while Custom CSS is off, so a user can prepare or change saved rules before enabling them. Its “Allowed CSS syntax” section lists the supported selector and property families and includes a valid example. Saved rules affect the board only while `customCSS` is true and `disableAll` is not true. Turning the setting off removes the owned constructed sheet without deleting the saved draft.

## Supported CSS

Selectors are limited to post content. The accepted spellings are:

- `.post` or `div.post`
- `.op` or `div.op`
- `.reply` or `div.reply`
- `.postMessage` or `blockquote.postMessage`
- `.postInfo`
- `.name`
- `.subject`
- `.quotelink` or `a.quotelink`
- `.fileText`

Comma-separated selector lists are allowed, up to eight selectors per rule. The module canonicalizes aliases and scopes them under `.board`. IDs, attributes, pseudo-classes, pseudo-elements, combinators supplied by the user, and selectors outside this list are rejected.

The allowed properties are `color`, `background-color`, `border-color`, `font-family`, `font-size`, `font-weight`, `font-style`, `line-height`, `text-align`, `letter-spacing`, `margin`, each margin side, `padding`, and each padding side.

Color values are three- or six-digit hexadecimal colors. Font families are limited to Arial, Helvetica, Arial/Helvetica/sans-serif, Verdana, Tahoma, Georgia, Times New Roman/Times/serif, monospace, sans-serif, and serif. Font size, weight, style, line height, text alignment, letter spacing, and spacing use fixed value sets in the module. Margin and padding shorthands may contain one to four allowed spacing values.

For example:

```css
.reply {
  background-color: #d6daf0;
  color: #34345c;
  padding: 4px 8px;
}

.postMessage {
  font-family: Arial, Helvetica, sans-serif;
  font-size: 14px;
  line-height: 1.4;
}
```

The parser rejects CSS escapes, comments, at-rules, variables, CSS functions, arbitrary selectors, duplicate properties in a rule, malformed braces or declarations, and any property/value pair outside the allowlists. That excludes external loads such as `url()` and `@import`, generated content, `display`, visibility/opacity controls, positioning, transforms, dimensions, z-index, pointer handling, and other rules that could hide or cover interface elements. Validation is all-or-nothing; one invalid declaration prevents the entire draft from being applied or saved by the editor.

## Limits and storage

Custom CSS is limited to 16,384 UTF-8 bytes, 64 rules, eight selectors per rule, 16 declarations per rule, and 256 declarations total. The shared `saveCustomCSS(raw, expected, signal)` adapter repeats the 16 KiB check before writing storage.

Saving uses the raw value that was read when the editor opened as a compare-and-swap expectation. The editor captures the submitted draft before waiting for the shared storage lock, so later typing cannot change either the saved value or the next comparison value. If another tab changes `4chan-css` before the save enters the lock, the adapter returns `conflict` and the editor leaves the newer value untouched. An empty successful save removes the storage key and changes the editor's next comparison value to missing storage.

Closing the editor or entering a persisted `pagehide` aborts an outstanding save. A save that started while Custom CSS was enabled is also aborted if the setting becomes disabled before the save completes. Disabling all native features aborts any outstanding Custom CSS save, including one that began while Custom CSS itself was off, and no new Custom CSS save starts while `disableAll` is true. Promise completion after any cancelled transition is ignored.

If browser storage or cross-tab locking becomes unavailable, the shared bootstrap keeps the successful value in its same-tab volatile cache. The editor reports that state and continues to use the new draft as the expectation for later same-tab saves. An empty draft removes `4chan-css` through the shared adapter.

The module listens for `4chan-css` and `4chan-settings` storage changes itself. A storage change refreshes the applied sheet, while an open editor keeps its current draft and relies on the expected-value check to catch stale saves. Duplicate mounts retire the previous controller before installing a new one.

On a persisted `pagehide`, the editor closes, pending work is aborted, and the owned sheet is removed. A persisted `pageshow` re-reads settings and storage before applying anything again. A final `pagehide` destroys the controller. Removing the mounted `.board` from the document also destroys the controller so a document-wide adopted sheet cannot outlive the board it belongs to. Teardown removes only the sheet instance and editor owned by this module and leaves other adopted stylesheets untouched.

Browsers without constructed stylesheet support do not receive an unsafe fallback. The editor and storage path remain available, but the saved rules are left unapplied.

## Verification

`tests/browser/native-custom-css.test.mjs` covers the parser, escape and dangerous-syntax rejection, constructed-sheet ownership, enable/disable behavior, BFCache restoration, duplicate mounts, stale conflicts, submitted-draft capture, clear-then-resave comparison state, global disable behavior, board detachment, and delayed save cancellation on close, disable, and pagehide.

`tests/browser/native-custom-css.spec.js` runs against the real public application. It checks the response CSP, verifies a valid stored rule through computed style, exercises the settings Edit action and real storage adapter, rejects an external-load draft without issuing a request, confirms rules stop applying when disabled, and confirms hostile stored CSS cannot trigger a stylesheet or image fetch.
