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
so the controller's CSS reads the same `--paper` value. If the optional theme
stylesheet fails, it uses the base page's `#ffffee` background. Hover and the
height/background controls are desktop options.

Hiding thumbnails makes them transparent and adds the reference's theme-colored
border. It preserves their place in the page and keeps expanded images visible.
It does **not** prevent thumbnail requests and is not a network-privacy setting.
An unrevealed spoiler shows the fixed public spoiler thumbnail and `Spoiler
Image` in place of its filename, without fetching the attachment. Spoiler
reveal restores the shortened filename and creates a lazy thumbnail from the
attachment's approved metadata; turning it off restores the label and original
placeholder. Filename filters still use the stored original name in either
state. Deleted attachments use the fixed public icon and have neither metadata
nor a loadable attachment control. Ordinary file links remain usable
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
keep the original spoiler label and fixed thumbnail. The
[file-state comparison](file-presentation.md) covers their generic public
presentation on OPs and replies, including both deleted-icon densities.

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

## Theme stylesheet failure

PR #188's second hosted Windows run passed 60 of 61 media cases. Its hover
background case found a transparent preview instead of the base page color;
the preview had the enabled background class. The retained screenshot was
downloaded and inspected. That job recorded script transport, but not stylesheet
transport, so it does not establish why the optional theme failed to apply.

A deterministic local case aborting only `/static/theme.css` reproduced the
same CSS failure: the document root retained `rgb(255, 255, 238)`, the body was
transparent and `--paper` was absent. The old variable fallback inherited the
body's transparency. Its replacement is the existing base page color. Loaded
themes still supply their own variable. The new case retains exact color and
healthy image-load assertions; six additional cases check each selected theme.
All 16 image-control cases then passed on Windows and Linux. This fixes the missing-theme
fallback, without claiming a cause for the hosted stylesheet failure or the
earlier unrelated Windows interaction failures.

The complete Windows media run passes all 68 cases, and the separate diagnostic
suite passes all five cases on both platforms, including the existing script
and privacy controls. The Linux image/diagnostic wrapper exits successfully.
The nine fixed-asset/CSP cases and formatting pass on Windows and Linux. Other
full-page Windows baselines are unchanged by this fallback fix.

```text
npx playwright test --config playwright.media-visual.config.js tests/media-visual/native-images.spec.js
```

Synthetic visual diagnostics now retain up to 16 stylesheet response
path/status pairs and 16 failed path/error-code pairs, plus finite computed
root/body/hover colors and an admitted hex page-color variable. They exclude
queries, fragments, headers, bodies, cookies and storage. No dependency,
media admission, worker authority, retry, timeout or pixel tolerance changes.

## Deleted media

Confirmed [mobile deletion](native-post-deletion.md) retires the exact validated
file URL for the page lifetime. Original and copied media cannot expand, hover,
or open through filename and thumbnail links after retirement. Pending images
are released; unrelated files retain their controls. The set holds at most
20,001 URLs. If it fills, enhancement fails closed with refresh guidance rather
than dropping old retirement records. Server-side file revocation remains the
authority after navigation or reload.
