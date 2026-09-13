# Catalog placeholder images and state icons

[Issue #66](https://github.com/frankischilling/26chan/issues/66) follows the
catalog cards and controls. The public v1025 client names `filedeleted-res.gif`,
`nofile.png` and `spoiler.png`. Public catalog CSS v705 names `sticky.gif` and
`closed.gif`, with `@2x` variants at two device pixels per CSS pixel.
[The asset manifest](public-catalog-assets.json) records the seven public URLs,
collection times, byte counts, SHA-256 hashes and dimensions. These are unchanged
public static images, not project-created artwork or files from the excluded
source checkout. No user posts, uploads, private source or production data were
collected. Board-specific spoiler images are outside this slice.

## Rendering and authority

The catalog uses the original no-file, deleted-file and generic spoiler images.
Measured public padding produces a 149×53 no-file box and a 155×53 deleted-file
box. The generic spoiler remains 100×100 in every size/teaser mode. Sticky and
closed indicators are 16×16 background icons over the thumbnail, using their
32×32 sources at high density. Alt text and labelled image roles describe each
state. The enclosing thumbnail link still opens the thread and retains its
keyboard focus indicator.

Seven literal GET/HEAD routes compile the release-owned bytes into the public
application. There is no filesystem lookup, upload endpoint, configurable asset
URL, runtime download or privileged image parser. Responses use the matching
PNG/GIF MIME, `nosniff`, and `public, max-age=0, must-revalidate`, without setting
cookies. Unknown paths return 404 and writes return 405.

The public image CSP names those seven exact paths plus the two existing theme
gradients. It does not allow all same-origin images or the upstream static host.
When development media is enabled, the separate configured media origin remains
an additional source. Public scripts remain disabled. A spoiler or deleted file
loads its UI image without requesting hidden or removed attachment bytes.
Serving these fixed reviewed GIFs does not enable GIF uploads or original-file
publication. Media decoding, storage grants and production disablement are
unchanged.

The board/thread spoiler details and deleted-file text are unchanged. Catalog
reveal preferences, board-specific spoiler selection, menus, watchers, original
teaser preprocessing and full page geometry remain incomplete under #6. An
unconfigured media origin still produces the explicit local "Image unavailable"
state. No whole-site or all-browser parity is claimed.

## Verification

The public reference can be reproduced without contacting the upstream host:

```text
node scripts/verify-public-catalog-assets.mjs .local/reference/catalog-20260913
```

The verifier validates all eight existing catalog source pins and seven UI image
pins, embeds the images into synthetic DOM/CSS, aborts browser network requests,
and never executes original client JavaScript. It passed 24 theme/viewport/scale
combinations, each covering all four catalog modes and the selected icon source.
The implementation's twelve corresponding browser cases passed at both widths,
all modes and both densities, checking loaded assets, accessible names, keyboard
focus, measured geometry, lack of upstream requests and hidden-media non-fetching.

These checks passed on Windows with the owned PostgreSQL 16.15 test cluster:

```text
cargo test -p board-public --test ui_assets --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1 --quiet
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo fmt --all -- --check
python scripts/check-media-parser-dependencies.py
```

All 57 public tests passed, including actual-router GET/HEAD hashes, MIME/cache
and CSP checks under both HTTP development and HTTPS production configuration,
unknown-route/write rejection, and real upload/post/delete browser workflows.
This is not deployed production qualification. The first full run caught an
upload browser selector that assumed every catalog image was user media. The
test now identifies the actual posted thread and also verifies that its deleted
catalog file loads the fixed placeholder without requesting either removed
media URL. No assertions were disabled. The failed run stopped before the final
upload test and clippy; the subsequent full run and clippy both passed.

Nine screenshot changes were individually inspected, then accepted with targeted
catalog-only updates. Every accepted PNG matches its reviewed capture byte for
byte. The other 36 baselines, browser pins, zero retries and zero-pixel tolerance
are unchanged. Final sequential runs passed all 65 fixture scenarios, including
45 screenshot comparisons, and all ten real-server browser scenarios:

```text
cargo build -p board-public --example visual-fixtures --locked
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:themes
npm run test:states
npm run test:behavior
```

The fixture suites used `VISUAL_FIXTURE_SERVER=1`; behavior tests used the
public application and the current owned database. `node --check` passed for
the reference verifier and all four changed browser test files. Source review
covered the compiled asset allowlist, routing, CSP, escaped template states,
reference pins and tests; no blocking finding remained.

The continuation's first database run loaded the old `.local/database.ps1` and
failed with `PoolTimedOut`. The current native PostgreSQL listener and restricted
public login were verified before loading its private environment. A second
attempt overlapped a running visual fixture and failed because Windows would
not replace its executable. After the fixture exited, the complete 57-test
public run and ten behavior cases passed sequentially. Neither failure required
an application change or weaker assertion.

Exact-head hosted CI is required before this slice can merge. Native Linux
containment and deployed production requirements remain separate from these
local rendering checks.
