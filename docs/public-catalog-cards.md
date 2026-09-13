# Catalog cards

Issue #62 covers the default compact catalog card, not the whole catalog client.
The reference is the permitted public v705 catalog CSS and v1025 client script,
recorded in [the source manifest](public-catalog-reference.json). The client
source establishes extended-small defaults, thumbnail-to-thread navigation,
`threads`, `thread-`, `thumb-`, `meta-` selectors, subject/teaser ordering and
visible reply/image-reply labels. No production posts or images were retained;
the original client was inspected as text and never executed.

Six public stylesheets, the mobile stylesheet and client script are pinned by
byte count and SHA-256. The local reproduction command loads those styles on
synthetic standards-mode HTML with all browser requests aborted:

```powershell
node scripts/verify-public-catalog-reference.mjs .local/reference/catalog-20260913
```

The Chromium pin and other browser/font environment details are recorded in
`reference-manifest.json`. Each theme is checked at 1280×900 and 390×900. The
observed default card is 180px wide, or 155px below 481px, with 320px maximum
content height, no panel border/background, and centered inline-block flow.
The metadata uses 11px text and an 8px line height. Thumbnail attributes are
bounded to 150px without upscaling; public CSS then floors both displayed axes
at 50px, including small images. These are synthetic computed-style facts,
not screenshots of the supplied old checkout or proof of whole-page parity.

## Application behavior

The actual HTML handler renders OP cards from the existing repeatable-read
board snapshot. Reply counts exclude the OP and deleted posts; image counts
exclude the OP, deleted replies and deleted files. Lifetime admission counts
are not substituted. JSON behavior and board-index previews are unchanged.
The existing concurrent-commit test continues checking coherent complete
responses rather than permitting mixed counts and content.

Thumbnails link to the thread, while full-image downloads remain on the
thread page. New media uses its normalized thumbnail; legacy approved media
without a thumbnail keeps its bounded full-image preview. Filenames, subjects
and the existing formatting representation remain escaped. Spoiler and
deleted-file cards contain no image URL, so they cannot request those bytes.

Explicit remaining differences: no-image, spoiler and deleted-file graphics
are currently text fallbacks; sticky/closed indicators are text. Sorting,
filtering and size/teaser settings have separate [control verification](catalog-controls.md).
Menus, watchers, hover previews and original teaser preprocessing remain
incomplete. Existing board-header
and page-navigation differences remain. Keyboard focus expands a clipped card
and preserves the visible focus outline so links remain accessible. Core
browsing needs no JavaScript, third-party code, cookies or new service access.
These limitations remain under #6, not a claim of complete visual parity.

## Verification record

The first Rust run caught the obsolete catalog assertion for “posts omitted”;
its replacement verifies the visible count of eight against a lifetime count
of nine. A newly added real-upload browser locator initially matched two
threads; it now targets the posted ID. The small-image test caught CSS auto
sizing producing 75×50 rather than the reference's 50×50 floor; bounded HTML
dimensions fixed it. The reference verifier rejected a mistyped mobile CSS
hash before loading it. Assertions and tolerances were not relaxed.

Database-backed media tests check catalog counts before deletion (7 replies,
7 image replies), after file deletion (7/6), and after reply deletion (6/5),
with a matching JSON control. Six browser cases check both viewports, shared
card styles, keyboard focus and actual thread navigation across all themes.
Media tests retain four loaded synthetic images, escaped filenames, exact
dimensions/URLs and no spoiler/deleted-media requests.

Five changed screenshots were individually inspected before targeted updates:
text catalog desktop, media catalog desktop/mobile, and empty catalog
desktop/mobile. Each updated PNG matches its reviewed capture byte-for-byte;
the other 34 baselines are unchanged. These remain local regression images.
Final local checks passed on owned Windows/PostgreSQL 16.15:

```text
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
npm run test:behavior
```

All 53 public tests and nine real-server browser scenarios passed. Sequential
ordinary visual suites (`test:visual`, `test:archive-visual`, `test:media-visual`,
`test:states`, `test:themes`, with `VISUAL_FIXTURE_SERVER=1`) passed 47 scenarios
and all 39 screenshot comparisons. Formatting, JavaScript syntax, the pinned
reference reproduction and credentialed-runtime decoder dependency guard
also passed.

PR #63 merged as `e0b5660beb191879fd2cef70b910db83bc7127d6` at
2026-09-13T07:01:44Z after final implementation review and a merge-preview tree
matching tested head `09aa7d06a5729e113690ea7388b7afa8dc086747`. PR build
34743287473 passed Linux qualification (18m35s) and Windows visuals (4m21s);
push build 34743285500 passed both (20m18s and 4m9s). Monitoring runs
34743287480 and 34743285515 passed. All native guest, dispatch, intake,
restoration, pressure and maintenance-alert steps succeeded. The advisory
workflow was path-filtered with no dependency changes.
No migration, dependency, service authority or production setting changes
are required.
