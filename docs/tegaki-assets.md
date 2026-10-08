# Tegaki assets

The [native drawing integration](native-drawing.md) uses the production Tegaki 0.9.4
bundle from the reference source. The older `js/tegaki-test.js` reports 0.9.1
and is not used.

`fixtures/tegaki-assets-reference.json` records the source revision, byte counts,
SHA-256 hashes, upstream URLs, notices and generated assets. Original inputs live
in `apps/public/vendor/tegaki/0.9.4`. Its local `.gitattributes` keeps the audited
CSS CRLF bytes intact across checkouts.

## Licenses and provenance

The upstream [0.9.4 tag](https://github.com/desuwa/tegaki/tree/0.9.4) resolves to
`b6697b164d6e9866adb39be4ef8c9db7102c8947`.

- Tegaki: MIT, Copyright (c) 2015 Maxime Youdine.
  The tag's unmodified license is in `docs/licenses/tegaki-MIT.txt`.
- Included UZIP.js: MIT, Copyright (c) 2018 Photopea.
  The tag's unmodified `lib/UZIP/LICENSE` is in
  `docs/licenses/tegaki-UZIP-MIT.txt`.
- Icon font: Font Awesome, Copyright (C) 2016 by Dave Gandy, and Entypo,
  Copyright (C) 2012 by Daniel Bruce, under SIL OFL. The tag's unmodified
  `fonts/LICENSE.txt` is in `docs/licenses/tegaki-font-NOTICE.txt`.
  Its Fontello configuration is retained alongside the vendor inputs.
- The complete SIL Open Font License 1.1 body is in
  `docs/licenses/tegaki-font-OFL-1.1.txt`. It was checked against the
  [official text](https://openfontlicense.org/documents/OFL.txt).
- The Fontello configuration also contains two custom glyphs: `spray-can-solid`
  (airbrush) and `pen-fancy-solid` (pen). Their geometry matches Font Awesome
  Free 5.7.2 after rescaling and upstream rounding. Tegaki does not identify the
  original Font Awesome Free release. The comparison SVGs and their hashes are
  pinned, and `docs/licenses/tegaki-fontawesome5-NOTICE.txt` records attribution
  and modifications. The unmodified Font Awesome Free license is in
  `docs/licenses/tegaki-fontawesome5-LICENSE.txt`: CC BY 4.0 applies to SVG paths,
  and SIL OFL 1.1 applies to the font glyphs.

These dependencies retain their own licenses; the surrounding reference
repository's license does not replace them.

The production CSS is identical to the upstream tag's CSS after normalizing
CRLF to LF. Its one embedded WOFF is extracted without changing the font. It
retains its `tegaki` name, glyphs and Fontello copyright metadata. The resulting
font is 3,128 bytes with SHA-256
`889ad3a092c684e62064431fe8f81ec948316a7875a3290525edb6bfc4af080b`.

## Deterministic preparation

Run `python3 scripts/prepare-tegaki-assets.py --check` to check all pinned inputs,
notices and outputs offline. Add `--reference /path/to/reference` to compare the
original JavaScript and CSS directly. Without `--check`, the same script writes
the three generated assets. It neither fetches dependencies nor compiles code.

The JavaScript transformation preserves the entire original byte sequence,
appends the complete Tegaki and UZIP MIT notices, and adds `export { Tegaki };`.
The CSS transformation normalizes line endings, replaces exactly one data URI
with `./tegaki-icons.v1.woff`, and appends the original font notice and complete
OFL text, plus the Font Awesome Free attribution and license for the two custom
glyphs. The required notices accompany the browser-delivered code and font
stylesheet as well as the source distribution.

## Integration contract

Import the named `Tegaki` singleton from
`/static/tegaki/tegaki-0.9.4.v1.js` and load
`/static/tegaki/tegaki-0.9.4.v1.css` before opening it. Serve the font at
`/static/tegaki/tegaki-icons.v1.woff` with `font/woff`.

The module reads `document.documentElement` during evaluation. All Tegaki and
UZIP references remain module-local; no `window.Tegaki` or classic-script load
ordering is required.

Use `open({width, height, onDone, onCancel, saveReplay: false, replayMode: false})`
for this integration. `flatten()` returns a new canvas containing the visible
layers. Finish hides the editor before calling `onDoneCb` and keeps those layers
available for export. `hide()` retains the attached editor and layers; `resume()` restores
them and binds input events. Close confirms, destroys the editor, then calls
`onCancelCb`. Guard `destroy()` with `if (Tegaki.bg)`.

Calling `open()` with an existing editor and matching `replayMode` only resumes
it. New dimensions and callbacks are ignored. A controller that retains one
session must manage ownership and update callbacks explicitly; opening another
form must not silently reuse another form's completion callback.

Pointer events go to `#tegaki-cursor-layer`. The painted layers sit above
`#tegaki-canvas`. Finish is `#tegaki-finish-btn`; the editor root is `#tegaki`.

## Content Security Policy

Add only the exact module URL to the page's existing script allowlist. The
stylesheet is covered by the existing `style-src 'self'`. Add only the exact
same-origin font URL to `font-src`; no `data:` font source is needed.

The audited bundle uses no eval, Function constructor, dynamic import, worker,
inline script, generated style element or `cssText`. It updates the individual
style properties `opacity`, `width`, `height` and `backgroundColor`. Those
CSSOM property changes do not require an `unsafe-inline` style allowance.

Ordinary drawing and `canvas.toBlob()` need no additional image or connection
authority. The source Open command decodes a local image and creates an object
URL without revocation. This integration disables Open and its file picker;
it adds no `blob:` image permission. Export uses a bounded PNG download wrapper
in `native-drawing-painter.js`. The wrapper owns one URL and
revokes it on replacement, dismissal, error or its 30-second expiry. Neither
operation adds blob script, font or worker authority.

The only `fetch()` is in the replay viewer. Replay is disabled in this integration,
and no replay connection authority should be added. Edit-from-post is also
outside this integration's scope.
