# Native Filters & Highlights Help

The native filter editor's `?` button opens a readable reference with semantic
headings and literal pattern examples. This is a presentation-only restoration
of `4chan-old/js/extension.js` lines 7227–7281 at the pinned reference revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The source compiler at lines 7118–7185
and the current runtime determine the wording where the old help is incomplete
or stale.

## Content and runtime checks

- Tripcode, Name and ID use exact, case-sensitive comparison. Tripcodes include
  their leading `!`; the example is `!Ep8pui8Vw2`.
- Comment, Subject and Filename are the pattern types actually offered. The old
  help's E-mail type is not offered.
- `feel` matches whole words without case sensitivity. `feel girlfriend` needs
  both words on the same line, in either order.
- `"that feel when"` is a case-sensitive substring, not a whole-field match.
  The native escaping list leaves `|` as alternation, including inside quotes.
- `feel*` and `idolm*ster` demonstrate wildcards. `*` cannot cross whitespace.
- Only `/.../` and `/.../i` select regular-expression parsing. The examples
  preserve `/feel when no (girl|boy)friend/i`, `/^(?!.*touhou).*$/i`, `/^>/`
  and `/^$/`. The NOT example is described for single-line text rather than
  promised as a multiline exclusion.
- Page comment matching treats missing comments as empty. Catalog discovery
  skips absent or empty raw comments; nonempty HTML that decodes to empty text
  can still match `/^$/`. These are separate execution paths.
- Color examples are `red`, `#0f0`, `#00ff00` and `rgba(34, 12, 64, 0.3)`.
  Help does not promise arbitrary CSS expressions: `filterColor` rejects
  declarations, variable references and inherited/context-dependent values.
- Board guidance uses lowercase slugs separated by spaces, with no slashes or
  leading separators. The parser does not normalize case and stops on an empty
  first token. Blank Boards is global for page filtering, but Auto needs
  explicit boards.
- Auto-watching searches catalog JSON during manual Thread Watcher refresh,
  with filtering and the watcher enabled. Subject filtering on pages is
  index-only. The `F` shortcut requires keyboard shortcuts and filtering.
- Filter Save and the filtering switch in Settings remain separate. The first
  active match wins. Bounded matching failures leave content visible. Hide
  normally supplies a View control; disabled thread stubs are an exception.

The current sources checked are `native-filter.js` (`compileNative`,
`boardTokens`, `matchesPrepared`, `matchesPage`), `native-filter-rules.js`
(`filterColor`), `native-page-filters.js`, `native-keybinds.js`, and
`static/thread-watcher.v1.js` plus `static/native-settings.v1.js`.

## Presentation and ownership

`client/native-filter-help.js` appends static text, `code` and `kbd` nodes without
HTML interpolation. The editor still owns the nested dialog, its Close button,
Escape handler and focus restoration. Opening or closing Help does not rebuild
rows, save preferences or discard an in-progress draft. The focusable title is
the initial focus target so long content opens at the top.

The heading hierarchy is:

- h2: Filters & Highlights Help
- h3: Tripcode, Name and ID filters
- h3: Comment, Subject and Filename filters
  - h4: Matching whole words
  - h4: AND operator
  - h4: Quoted strings
  - h4: Wildcards
  - h4: Regular expressions
- h3: Colors
- h3: Boards
- h3: Auto-watching
- h3: Shortcut
- h3: Applying filters

Help-only CSS supplies comfortable line height, compact spacing and wrapping.
The existing viewport-constrained dialog scrolls vertically. Desktop-only
editor entry points and the separate catalog filter help are unchanged.

## Verification scope

This change does not modify compiler semantics, filtering, storage, watcher
discovery, settings gates or palette behavior. Browser regression coverage
checks headings and literal examples, draft retention through Help,
Close/Escape focus return, repeat opens, small viewport scrolling and the
unchanged mobile/catalog gates. The 17 filter unit cases and all 167 watcher-core
cases pass locally. A fresh migrated database run passes 126 public library and
two binary tests; the known executor-blocked Unix-socket case remains enabled
in CI. Formatting and strict workspace Clippy pass. Generated assets match their
pinned sources; the filter
bundle is 261,144 bytes, below its 256 KiB ceiling. Rendered browser qualification
remains pending because the local executor cannot launch Chromium.
