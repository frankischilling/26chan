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
Revealed spoiler captions also retain the filename tooltip through their
registered image controller. Quote copies cannot gain that ownership. The
existing tooltip lifecycle clears timers and descriptions when the post
changes, leaves the page, becomes hidden, or loses its validated recipe.

Filename filters read the full title, rather than the shortened link. Menus,
spoiler reveal, local/remote quote copies and live updater posts use the same
file header. The finite parser binds its post ID, caption, filename, thumbnail
dimensions and media targets before construction. The generated parser remains
inside its 262,144-byte release ceiling.

Unrevealed spoilers show the unchanged public 100-by-100 `spoiler.png` asset,
with a `fileThumb imgspoiler` link, mobile size caption and full filename title
on the file header. This loads no attachment media. A direct click opens the
approved file; enabling reveal replaces the placeholder with an approved lazy
thumbnail and the shortened filename. The revealed thumbnail fits a 250-pixel
OP or 125-pixel reply box. Turning reveal off restores the original placeholder.
Expansion and hover do not fetch an unrevealed spoiler implicitly.

Deleted files use a non-link `span.fileThumb` and the unchanged public deleted
icon. Its 127-by-13 CSS dimensions follow the released client. A finite `srcset`
selects the 254-by-26 asset at density 2. Deleted posts expose no filename, file
link or attachment request. Both states survive updater reconstruction and
ID-free quote preparation without acquiring arbitrary asset paths or callbacks.

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

The [file-state reference](public-file-states-reference.json) adds 20 released
formatter cases and 192 style cases: spoiler/deleted files, OPs/replies, six
themes, both mobile families, desktop/mobile viewports and densities 1 and 2.
The source formatter supplies the synthetic file DOM for style collection.
Only the three unchanged, hash-checked public placeholder assets are embedded;
all external requests are denied. Production-template comparisons use the same
properties, and density tests check the actual selected deleted-icon URL.

The existing media policy still publishes normalized PNGs and retains uploaded
filenames as display metadata. The mobile format caption describes those PNG
bytes, even when the display filename has another extension. Original downloads,
animated media and full original-page matching remain unfinished. Board-specific
custom spoiler selection and original server filename acceptance remain
unqualified. The generic placeholder comparison does not establish those rules.

The Windows board, thread and archived-page captures changed on desktop and
mobile. All six received images were inspected before their individual baseline
updates. Catalog captures were unchanged.

```powershell
node scripts/verify-public-file-reference.mjs .local/references/extension.1191.js
node scripts/verify-public-file-style-reference.mjs .local/references
node scripts/verify-public-file-states.mjs .local/references
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
