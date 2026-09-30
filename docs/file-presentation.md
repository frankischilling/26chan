# File headers and mobile captions

Public attachments use the released client's `fileText` header and `fT` post
identifier. Sizes follow its byte, kilobyte and two-decimal megabyte rules.
The dimensions use `widthxheight`. Names longer than 40 UTF-16 units on an OP,
or 30 on a reply, keep the first 35 or 25 units and their extension. Shortened
links retain the complete escaped filename in their native title.

At widths up to 480 pixels, the file header is hidden and the thumbnail has a
compact size/format caption. The thumbnail box has independent 125-pixel width
and height limits; `object-fit: scale-down` preserves the drawn image's ratio.
Desktop thumbnails retain their approved dimensions. The exact mobile opt-out
restores the desktop presentation. The picture-hiding setting also hides the
compact caption.

Hovering an original mobile caption shows its complete filename after 300 ms.
The tooltip uses text nodes. Filename text cannot invoke callback names or
create HTML, and copied quote captions cannot become original-post actions.
The existing tooltip lifecycle clears timers and descriptions when the post
changes, leaves the page, becomes hidden, or loses its validated recipe.

Filename filters read the full title, rather than the shortened link. Menus,
spoiler reveal, local/remote quote copies and live updater posts use the same
file header. The finite parser binds its post ID, caption, filename, thumbnail
dimensions and media targets before construction. The generated parser remains
inside its 262,144-byte release ceiling.

## Evidence and limits

[Forty formatter vectors](public-file-reference.json) come from the pinned
[public v1191 client](https://s.4cdn.org/js/extension.min.1191.js), with escaped
synthetic filenames and bounded numeric metadata. They include size boundaries,
entity-looking names and a cutoff that splits a surrogate pair. Rust emits the
replacement character at a split surrogate so the response remains valid UTF-8.

[Twenty-four style cases](public-file-style-reference.json) use eight pinned
public stylesheets, six themes, both mobile families, 1280 by 900 and 390 by 844
viewports, and Chromium 151.0.7922.34 at density 1. All requests are denied during
collection. Production-template tests compare the same properties. These facts
qualify file presentation, not original full-page pixels or server filename
acceptance. The script's initial synthetic anchors lacked `href`, which omitted
link-state styling; the collected facts were corrected before acceptance.

The existing media policy still publishes normalized PNGs and retains uploaded
filenames as display metadata. The mobile format caption describes those PNG
bytes, even when the display filename has another extension. Original downloads,
animated media and full original-page matching remain unfinished. Spoilers keep
the existing script-free disclosure form, and deleted files keep the existing
text state. Those two presentations are outside this reference comparison.

The Windows board, thread and archived-page captures changed on desktop and
mobile. All six received images were inspected before their individual baseline
updates. Catalog captures were unchanged.

```powershell
node scripts/verify-public-file-reference.mjs .local/references/extension.1191.js
node scripts/verify-public-file-style-reference.mjs .local/references
npm run test:files-core
cargo test -p board-public --lib --test ui_assets --locked
cargo clippy -p board-public --all-targets --locked -- -D warnings
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:media-visual
npm run test:themes
Remove-Item Env:VISUAL_FIXTURE_SERVER
```

The ignored reference directory must contain the exact public files identified
by the manifests. The replay scripts check their hashes before using them.
Linux persisted posting, role and containment checks run in the PR workflow;
local fixture pages do not establish those boundaries.
