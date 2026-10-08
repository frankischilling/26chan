# Native PNG drawing

This partial implementation of [#207](https://github.com/frankischilling/26chan/issues/207)
adds Tegaki to ordinary OP/reply forms and Quick Reply through the existing
[isolated upload and approval workflow](post-attachments.md). The drawing
workflow and the source-parity target in
[PR #227](https://github.com/frankischilling/26chan/pull/227) remain incomplete.

## Source and board policy

The reference is `4chan-old` revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. `PainterCore` in `js/core.js`,
the painter methods in `js/extension.js`, and the Draw row in
`views/imgboard.php` define the ordinary and Quick Reply controls.
`tests/fixtures/native-drawing-source.json` pins their source bytes and the
relevant Tegaki lifecycle functions. Its Node oracle executes those extracted
functions with inert stubs; it does not render drawings.

Migration 0114 imports `ENABLE_PAINTERJS`, `ENABLE_OEKAKI_REPLAYS` and
`PAINTERJS_DIMS` into `oekaki`, `oekaki_replays`, `oekaki_width` and
`oekaki_height`. The inventory fixture covers all 82 boards. Drawing is enabled
in the source on `/i/`, `/qst/` and `/vip/`; replay is enabled only on `/i/`.
Every board inherits the 400 x 400 default. The drawing declarations in
`test.config.ini` are commented out.

The runtime currently supports only non-replay drawing. `/i/` keeps its
imported policy but exposes neither an editor nor an `oekaki` API capability
until replay support is implemented. `/qst/` and `/vip/` also require an
already-enabled media profile, a public non-text-only board and a positive
image limit. Board and open-thread forms expose desktop Draw controls;
catalogs, closed/archived threads and error pages do not. `boards.json`
advertises `oekaki: 1` only when the supported board/media gates pass. This
feature does not enable production uploads.

The source form handlers reject dimensions below one. The `/i/` declarations
`OEKAKI_MIN_W/H = 100` and `OEKAKI_MAX_W/H = 800` have no executable consumers
in the supplied checkout. The rewrite's 1..1024 whole-pixel dimensions and
1,048,576-pixel ceiling are isolated-decoder safety limits, not those source
constants. They also bound Tegaki's New command before allocation.

## Editor and attachment lifecycle

The local editor uses the pinned production Tegaki 0.9.4 bundle. The older
0.9.1 test script is unused. [Asset provenance and licenses](tegaki-assets.md)
cover the unchanged vendor inputs, deterministic module/CSS/font preparation
and notices.
The module loads on the first Draw action. Ordinary drawing remains available
independently of the extension's disable-all setting; Quick Reply follows its
own availability rules.

Finish hides the editor, retains its layers and exports `tegaki.png`. Draw
becomes Edit, canvas dimensions lock and the ordinary file picker is hidden.
Form Clear releases the upload and retains the canvas. The editor's Close
command confirms before destroying it. Reopening rebinds the
completion callbacks because Tegaki resumes an existing canvas before reading
new options. A pending drawing in another form requires confirmation and
successful upload cancellation before replacement.

PNG export checks dimensions, MIME, signature and the 8 MiB upload ceiling
within a ten-second deadline. These checks do not authorize publication:
the PNG still passes through intake, isolated decoding, normalized-media
approval and transactional one-use attachment consumption. Text and layers
remain available after failure. Late export, upload and status completions are
bound to their form, target and generation. Automatic status checks are bounded;
Check status remains available. Posting prevents replacement. An uncertain
posting response retires its capability so it cannot be reused.

The editor's Open command and file picker are disabled because the source
importer decodes a local image without the integration's bounds and never
revokes its object URL. Export remains available through a bounded PNG download wrapper.
It owns one download URL, revokes it on replacement, dismissal or failure,
and expires it after 30 seconds. A late export cannot create a URL after its
editor session has been invalidated.

Only a successfully rendered drawing form receives CSP permissions for the exact
local Tegaki module and font. No blob image permission, remote asset source
or replay fetch permission is added. Browser PNGs gain no approval authority.

## Qualification and remaining work

Apply migration 0114 before starting binaries that read the drawing columns.
The additive migration preserves existing content and authority; a binary
rollback can retain the columns but loses the new editor support.

Local qualification passed strict workspace Clippy, generated-asset checks,
the populated 0114 upgrade, four drawing source/policy/asset/renderer tests,
128 public library tests, 37 focused Node cases and 13 drawing-supervisor tests.
The sandbox-denied Unix-socket library test remains in hosted CI. The upgrade
check covers all source policies, future defaults, historical rows, operator
edits, private-board visibility and seven runtime-role write denials. Node
cases cover the source oracle, PNG bounds, editor ownership, cancellation races
and download URL lifetime.

A disposable PostgreSQL check also verified that receiving competitors prevent
dispatch, another reservation blocks while the queue gate is held, and an
interrupted gate releases its lock. That local check used SQL reservations;
it does not establish the separate authenticated HTTP or VM workflow. None of
these local results establishes a browser pass.

`native-drawing.spec.js` checks media-disabled page gates and delivered assets.
The separate `drawing-upload.mjs` suite is designed to exercise real Tegaki
pointer strokes, retained canvases, PNG downloads, receipt-bound isolated
approval, persisted posting, normalized pixels, capability reuse denial and
owned deletion. Its supervisor supplies isolated processing rather than
synthetic approvals. Actual browser execution and exact-head hosted CI remain
pending.

Replay recording, validation, storage, delivery and playback remain unfinished,
as do edit-from-post and source drawing annotations. The original annotation
path in `imgboard.php:6064-6097` and `lib/oekaki.php` is inside the replay-enabled
branch and uses `oe_time`, replay metadata and validated `oe_src` to append the
Oekaki Post time/replay/source text. This integration does not send or reproduce
that metadata. Safe image import, complete desktop/mobile source comparison and
browser qualification are also open requirements. #207 remains incomplete.
