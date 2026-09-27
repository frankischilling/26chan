# Native image controls

Board and thread pages expand an approved image when its thumbnail is clicked.
A second click restores the thumbnail. Settings now includes the public client's
Images & Media group: image expansion, fitting to the screen, hover previews,
backgrounds for transparent previews, spoiler reveal and thumbnail hiding.
The catalog keeps its existing display preferences.

## Reference and behavior

The reference is the publicly served `extension.min.1191.js`, also used by the
link, quote and backlink features. Its SHA256 is
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
The [reference manifest](public-image-controls-reference.json) records the source
and the checks against its image functions. No upstream script or public post
content is copied into application fixtures.

Expansion is enabled by default; the other image preferences are off. Expanded
images keep their aspect ratio and do not grow beyond their native size. Width
fitting reserves 25 pixels to the right of the original thumbnail. The optional
height limit is the viewport height. Hover previews use one fixed image at the
top right, leave 20 pixels beyond the hovered thumbnail and fit within the
viewport. The preview background option uses the theme's page color. The public
client inherits that color from its body; our themes put it on the document root,
so the controller's CSS reads the same `--paper` value. Hover and the
height/background controls are desktop options.

Hiding thumbnails makes them transparent and adds the reference's theme-colored
border. It preserves their place in the page and keeps expanded images visible.
It does **not** prevent thumbnail requests and is not a network-privacy setting.
An unrevealed spoiler shows `Spoiler Image` in place of its filename. Spoiler
reveal restores the original filename and creates a lazy thumbnail from the
attachment's approved metadata; turning it off restores the label and original
spoiler details. Filename filters still use the stored original name in either
state. Deleted attachments have
neither metadata nor a loadable image control. Ordinary file links remain usable
when JavaScript or the extension is disabled.

The controller also handles inserted posts and removes images when their source
is hidden, deleted, replaced or detached. Settings changes, page suspension and
return from the back-forward cache update the same controller. Temporary images
belong to the shared projection registry, so quote copies retain the original
thumbnail and cannot copy an expanded image into another post.

## Media and resource limits

Only an exact normalized PNG route on the configured media origin can be opened
by an image control. Credentials, query strings, fragments, path rewriting,
foreign origins, alternate formats and out-of-range identifiers are rejected.
The source thumbnail must belong to the same approved file. Spoiler metadata
contains bounded dimensions and the original filename, not another URL to trust.
Filenames retain the store's 255-byte UTF-8 limit and exclusion of control
characters. Temporary filename labels are projection-owned, so quote copies
keep the original spoiler label.

The released client polls image completion without a deadline and can keep
multiple images open. This implementation admits at most eight expanded images
and one hover preview. Each full-image load has a ten-second deadline. Errors,
timeouts and cancellation remove the pending element and release its slot; a
failed expansion leaves the thumbnail available for an explicit retry. A page
can reveal at most 1,001 spoiler thumbnails, matching the existing finite post
snapshot bound, and those thumbnails retain native lazy loading.

These limits bound optional browser work. They do not substitute for the media
publication boundary: the browser still decodes the approved PNG. The current
publisher limits both dimensions to 1,024 pixels and never exposes raw uploads.
Original GIF, JPEG and WebM delivery, original-site full-page parity, arbitrary
comment-link media previews and complete quote-spoiler presentation remain
outside this slice. Exact adaptation of the public client's small board-index
expansion offset also needs comparison with our different thread-hiding layout.
Production media qualification remains tracked in #5.

The controller is a separate fixed module, `/static/native-images.v1.js`, with a
16 KiB build ceiling. The page CSP names that route explicitly. It has no imports
or worker-network authority, cannot be served from the API listener, and has no
write route. Settings reuse the existing bounded `4chan-settings` storage.

## Verification

Run the source and bounded DOM tests with:

```sh
npm ci --ignore-scripts
npm run test:images-core
cargo test -p board-public --test ui_assets --locked
node scripts/verify-public-image-controls.mjs /path/to/extension.min.1191.js
```

The DOM tests use synthetic PNG responses, held requests and failures. They check
admission, deadlines, explicit recovery, source invalidation, suspended pages,
settings changes, URL rejection and original quote ownership. The media browser
suite uses the production templates and released module with synthetic approved
attachments:

```sh
cargo build -p board-public --example visual-fixtures --locked
npx playwright test --config playwright.media-visual.config.js tests/media-visual/native-images.spec.js
```

The implementing PR records the executed browser, database and CI checks. These
tests establish the implemented image controls and their limits, not complete
visual parity with an original public page. Issue #180 tracks this feature group
within the broader reference work in #6.
