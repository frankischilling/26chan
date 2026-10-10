# Native PNG drawing

This partial implementation of [#207](https://github.com/frankischilling/26chan/issues/207)
adds Tegaki to ordinary OP/reply forms and Quick Reply through the existing
[isolated upload and approval workflow](post-attachments.md). The drawing
workflow and the source-parity target in
[PR #227](https://github.com/frankischilling/26chan/pull/227) remain incomplete.

## Source and board policy

The reference is the supplied, ignored, read-only local `4chan-old/` folder at
revision `545b7812d1849f7958d914950c91fdbbe38f6b22`; a fresh checkout does
not contain it. `PainterCore` in its `js/core.js`, the painter methods in
`js/extension.js`, and the Draw row in `views/imgboard.php` define the ordinary
and Quick Reply controls.
`tests/fixtures/native-drawing-source.json` pins their source bytes and the
relevant Tegaki lifecycle functions. Its Node oracle executes those extracted
functions with inert stubs; it does not render drawings.

Migration 0114 imports `ENABLE_PAINTERJS`, `ENABLE_OEKAKI_REPLAYS` and
`PAINTERJS_DIMS` into `oekaki`, `oekaki_replays`, `oekaki_width` and
`oekaki_height`. The inventory fixture covers all 82 boards. Drawing is enabled
in the source on `/i/`, `/qst/` and `/vip/`; replay is enabled only on `/i/`.
Every board inherits the 400 x 400 default. The drawing declarations in
`test.config.ini` are commented out.

Ordinary non-replay Draw remains available on media-enabled `/qst/` and `/vip/`
board and open-thread forms. [Drawing Edit on `/i/`](drawing-edit.md) separately
imports approved post PNGs into Quick Reply on live threads. Blank-canvas
`/i/` Draw and replay support remain disabled. Drawing controls require an
eligible public, non-text board with image capacity; closed or archived threads,
catalog pages and missing threads do not expose Edit. `boards.json` reports the
imported `oekaki` policy independently of runtime media availability. These
controls do not enable production uploads.

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

The [Drawing Edit slice](drawing-edit.md) now imports approved post PNGs and
persists optional `oe_time` and `oe_src` annotations as typed fields. Public
views, search and staff reports display the generated Oekaki Post text; new
moderation audit snapshots retain the fields separately from the saved comment.
The corresponding original rules are in the local reference paths
`4chan-old/imgboard.php:6063-6097` and `4chan-old/lib/oekaki.php:102-160`.
Replay recording, validation, storage, delivery and playback, ordinary `/i/`
Draw, replay links and complete desktop/mobile source comparison remain open.
Current-head browser and CI qualification for this Edit slice is separate;
[#207](https://github.com/frankischilling/26chan/issues/207) remains incomplete.
