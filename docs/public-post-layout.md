# Referenced desktop post layout

[Issue #58](https://github.com/frankischilling/26chan/issues/58) narrows the
layout gap using the six public v716 stylesheets already pinned in
[the theme manifest](public-theme-reference.json). The recorded
[post-style facts](public-post-layout-reference.json) cover desktop post display,
padding, comment margins and line height, thumbnail floats/margins, reply arrows,
and each theme's normal and targeted reply borders.

The public `/po/` page was read once on September 13, 2026. Its response hash,
byte count and collection time are recorded in the post-style manifest. An
allowlisting parser inspected structural tags and classes only. The OP file
preceded its desktop header; arrows preceded the reply. The inspected reply was
text-only, so this collection does not establish reply-file order. No response
body, production post text, identifiers, filenames, links or user images were
retained as fixtures. The unverified `4chan-old` checkout was not used.

`tests/themes/reference-post.html` is small, project-owned synthetic markup.
The measurement command loads it with each locally retained public stylesheet,
checks the stylesheet's byte count and SHA-256 first, and aborts every browser
network request. It checks the recorded facts, including hash-target borders:

```powershell
node scripts/verify-public-post-reference.mjs .local/reference/themes-20260913
```

The command passed for all six sources with pinned Chromium 151.0.7922.34 at
1280 by 900, scale 1. It requires the separately collected CSS directory and
does not download anything. The regular Windows CI suite instead compares the
actual application styles against the committed facts in six browser cases.
Neither check is an original rendered-page comparison or independent audit.

## Implementation and scope

Board and thread posts now use the measured desktop flow. Reply arrows are
decorative and hidden from accessibility APIs. The OP attachment precedes the
post header; reply and catalog attachment order is preserved. A shared Askama
macro retains escaping, file-deleted markers, spoiler links without image
requests, normalized/legacy thumbnails, lazy loading and separate-origin links.
It renders at exactly one of the two mutually exclusive call sites per post.
Posting, deletion/report forms and media authority are unchanged.

At widths up to 600 pixels, images remain stacked and comments use the existing
smaller margins. Reply width reserves space for arrows according to font size.
These are explicit local adaptations, not measured original mobile behavior.
Long filenames still wrap instead of using the public stylesheet's nowrap
limit. Action forms clear floated images so they remain usable. Forms, catalog
geometry, original client scripts and complete page geometry remain outside
this change and unresolved under #6; production qualification remains under #5.

## Review and regression history

The first application property test failed: the OP was `block` rather than the
reference's `inline`. The attachment extraction initially missed Askama's
`endcall` terminator; fixing the template restored compilation. Four of six
theme property cases then passed, while Tomorrow and Photon exposed the old
`border-style: none` on top/left edges. Explicit solid border style with the
measured per-theme widths made all six cases pass.

Screenshot review caught cramped mobile text beside floated images. Disabling
thumbnail and image floats at the mobile breakpoint restored readable stacked
content. A second review caught serif-theme arrows wrapping above replies;
font-relative reserved space fixed that. Browser assertions now check image
float behavior, intrinsic dimensions, arrow alignment, overflow, file escaping,
spoiler/deleted-file request absence and existing navigation/cookie behavior.
The screenshot assertions collect all theme differences using soft assertions;
any mismatch still fails the test. No pixel tolerance or retry was increased.

All final changed captures were inspected before baseline updates: two board,
two archived-thread, six attachment and twelve theme images. Ten of the final
theme captures were byte-identical to the prior reviewed captures; the two
corrected serif mobile captures were inspected separately. These are synthetic
application regression baselines in the recorded Windows/font environment,
not production user content or full original-site parity evidence.

The public all-feature suite passed 52 tests, including actual restricted
database routes and the approved-attachment browser flow; clippy passed with
warnings denied. An overlapping local browser launch hit the fixture's port
3000 and aborted before testing. Browser suites must run sequentially, with
server reuse disabled. The sequential normal-server rerun passed all nine
scenarios. Final ordinary visual runs passed 35 scenarios with 39 screenshot
comparisons: states 10, board 3, archive 6, attachment 8, and themes 8 (six
property cases plus two scenarios containing twelve screenshots). Exactly 22
baselines changed and matched the inspected captures byte-for-byte; the other
17 remained unchanged. Formatting, JavaScript syntax, whitespace and the media
parser dependency guard also passed. Exact-head hosted CI remains required
before merge.

```powershell
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
npm run test:behavior
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:states
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:themes
node scripts/verify-public-post-reference.mjs .local/reference/themes-20260913
cargo fmt --all -- --check
python scripts/check-media-parser-dependencies.py
```

No dependency, migration, credential, workflow permission, production media
setting, deployment or release change is included. Parent #57 is merged; its
completed verification is recorded in [the public-state notes](verification-public-states.md).
