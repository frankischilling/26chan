# Drawing Edit on /i/

This [#207](https://github.com/frankischilling/26chan/issues/207) slice adds the
source's existing-image **Edit** action to live `/i/` thread pages. It opens an
approved post image in Quick Reply's Tegaki 0.9.4 editor, then posts the edited
PNG through the existing isolated attachment workflow. The earlier
[ordinary PNG drawing integration](native-drawing.md) remains available on its
supported non-replay boards. Full `/i/` drawing and replay support is still open.

## Source and page eligibility

The pinned `4chan-old` source revision is
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Its
`js/extension.js:1243-1244,1288-1307` adds Edit to eligible post files on an
Oekaki thread. `QR.onOpenInPainterClick` at lines 3831-3905 looks up an exact
`a[class="fileThumb"]`, loads its image anonymously, and opens Tegaki with
`saveReplay: false`. `QR.onPainterDone` and `QR.submit` at lines 3933-3950 and
4671-4683 supply elapsed `oe_time` and an optional `oe_src`. The extracted
functions are pinned in
[`tests/fixtures/native-drawing-source.json`](../tests/fixtures/native-drawing-source.json)
and checked by
[`scripts/record-drawing-edit-source.mjs`](../scripts/record-drawing-edit-source.mjs).

The rewrite requires the literal board slug `i`, both imported `oekaki` and
`oekaki_replays` policy, enabled public media, a positive image limit, a public
non-text board and supported board dimensions. Edit appears on a real, open,
unarchived thread, with no controls on the index, catalog, missing thread or
closed thread. Ordinary blank-canvas **Draw** remains disabled on `/i/`.
[`Board::drawing_edit_enabled`](../crates/store/src/lib.rs),
[`BoardPage::drawing_edit_allowed`](../apps/public/src/views.rs) and
[`board.html`](../apps/public/templates/board.html) enforce the server gate.
The [drawing policy tests](../apps/public/tests/drawing_policy.rs) cover the
separate Draw and Edit decisions.

[`native-drawing.js`](../apps/public/client/native-drawing.js) adds Edit beside
a live thread post's file text and refreshes those links after updater or
thread-state changes. It checks the matching post container, numeric ID, exact
media origin and canonical full `/i/{tim}.png` URL. Spoiler anchors carrying
extra classes fail the source's exact `fileThumb` check. Approved JPEG uploads
use their normalized PNG representation; Edit does not fetch the displayed
`s.jpg` thumbnail.

## Image import and retained Quick Reply state

The [media reader](../apps/media-http/src/serving.rs) checks current approved
post-media metadata and verified bytes before replying to a full PNG request.
Its GET/HEAD post-image responses, including a verified 304, use `Vary: Origin`.
Only one `Origin` header exactly equal to the configured public origin receives
`Access-Control-Allow-Origin`. No credentialed CORS header is issued. Missing or
unapproved files, GIFs, thumbnails and the generic `/media/` endpoint receive
no drawing-import CORS permission. See the
[CORS tests](../apps/media-http/tests/cors.rs).

[`native-drawing-painter.js`](../apps/public/client/native-drawing-painter.js)
loads that full image through `Image` with `crossOrigin = 'anonymous'` and
`referrerPolicy = 'no-referrer'`. It stops failed or pending loads after ten
seconds. Decoded width and height must each be 1 through 1,024 pixels, with no
more than 1,048,576 pixels total. The importer hands the image to Tegaki only
after those checks. Tegaki's unrestricted local-file Open path stays disabled.
Finish exports a checked `tegaki.png` within the 8 MiB upload bound; the PNG
still needs intake, isolated decoding, approval and one-use post consumption.
These limits come from [native drawing core](../apps/public/client/native-drawing-core.js),
not the unused source `OEKAKI_MIN/MAX` constants.

Quick Reply and the ordinary form share one retained Tegaki engine. Finish and
form Clear keep its layers; reopening the same form rebinds completion callbacks.
Importing another image or replacing a different form's canvas asks for consent
and cancels the old upload before switching ownership. Closing Quick Reply
retires its upload state while preserving the canvas for a possible reopen;
changing thread deliberately retires that retained owner. Async image loads,
export, status checks and receipts carry generation and target checks, including
on pagehide/BFCache. An uncertain posting result retires the one-use capability.
The lifecycle lives in
[`native-drawing.js`](../apps/public/client/native-drawing.js),
[`native-drawing-painter.js`](../apps/public/client/native-drawing-painter.js)
and [`native-quick-reply.js`](../apps/public/client/native-quick-reply.js).

## Post metadata and display

The browser sends `oe_time` and `oe_src` only for its finished, approved imported
Edit. Time is rounded elapsed drawing time, rather than a verified duration.
`imgboard.php:6063-6097` and `lib/oekaki.php:102-160` append source-style
**Oekaki Post** information within the accepted-image, replay-enabled branch,
resolving the optional source before formatting. Migration
[`0126`](../migrations/0126_post_drawing_annotations.sql) instead keeps
`drawing_time_seconds` and `drawing_source_post_id` as bounded nullable post
columns. Time must be 1 through 5,184,000 seconds (60 days). A source must
identify an eligible imaged post on the same board: any OP or a reply in the
target thread with a nonzero image `tim`. Invalid or missing source leaves a
time-only annotation. The locked accepted-attachment transaction stamps these
fields without changing the saved comment, duplicate-admission hashes or
wordfilter search cache. A source ID records a submitted reference and its
eligibility at posting time; it does not establish that those pixels were used.

[`DrawingAnnotation`](../crates/domain/src/drawing_annotation.rs) generates
the source's `<small><b>Oekaki Post</b>` suffix, including independently rounded
minutes and plain `Source: >>ID` text. Thread fragments, JSON, catalog,
archive, RSS, search excerpts and semantic thread text reuse that projection.
Local quote previews retain the annotation and approved file presentation while
omitting the temporary Edit control from the copied post.
The public search SQL in [`read.rs`](../crates/store/src/read.rs) also includes
the plain annotation words when selecting matching threads and replies;
[`drawing_search_text`](../migrations/0126_post_drawing_annotations.sql)
does not alter stored comments. Staff reports read the same typed fields and
render the same small/bold text with an unlinked source number through
[`store.rs`](../apps/staff/src/store.rs),
[`views.rs`](../apps/staff/src/views.rs) and
[`queue.html`](../apps/staff/templates/queue.html).

Migration [`0127`](../migrations/0127_moderation_drawing_snapshots.sql) adds
nullable drawing time/source fields to immutable moderation snapshots. New
thread-options, spoiler/unspoiler and force-archive audits capture them as
`snapshot_version = 2`, retaining the exact raw `snapshot_comment`. Historical
unversioned and version 1 snapshots remain valid and receive no backfill. The
existing bounds, action restrictions and append-only staff privileges remain.
See [`audit_snapshot.rs`](../apps/staff/src/audit_snapshot.rs) and the
[snapshot tests](../crates/store/tests/moderation_target_snapshots.rs).

## Qualification and remaining scope

These are checks for this slice, with database tests requiring a migrated,
disposable test database and the configured role-specific connections. They are
listed for qualification; this document makes no current-head test or CI pass
claim.

```bash
node scripts/record-drawing-edit-source.mjs --source /path/to/4chan-old --check
npm run test:drawing-core
python3 -m unittest discover -s tests/media -p 'test_public_drawing_fixture.py'
python3 -m unittest discover -s tests/media -p 'test_public_upload_fixture.py'
cargo test -p board-public --features database-tests --test drawing_policy --test drawing_authority --locked
cargo test -p board-media-http --features database-tests --test cors --locked
cargo test -p board-store --features database-tests --test drawing_search --test post_media --test moderation_target_snapshots --locked
cargo test -p board-staff --features database-tests --test moderation_snapshots --test force_archive --locked
```

The separate guarded lease check requires `MIGRATION_DATABASE_URL` for an idle,
disposable, migrated database whose `/i/` board has no existing posts or private
admission state:

```bash
cargo build -p board-public --example drawing-lease-format --locked
DRAWING_EDIT_LEASE_DISPOSABLE=1 python3 scripts/test-drawing-edit-lease.py
```

The helper formats the synthetic posts under the original `/i/` wordfilter
policy. It receives only bounded text and formatting flags over stdin. If the
build uses a separate target directory, set `DRAWING_LEASE_FORMATTER` to the
absolute path of its `debug/examples/drawing-lease-format` executable.

The full browser/media check runs `scripts/test-media-intake.sh` as root on the
owned disposable Linux host after its Firecracker, intake and browser
prerequisites are provisioned. It requires `MEDIA_VM_TEST_CONFIG`,
`MEDIA_VM_PROBE_CONFIG` and `PUBLIC_UPLOAD_NODE`. It runs the
original upload cases, then ordinary/Quick Reply drawing and the `/i/` Edit
browser case. The Edit fixture in
[`public_drawing_fixture.py`](../tests/media/public_drawing_fixture.py)
leases only an untouched disposable `/i/`, restores the original board policy
after owned cleanup and refuses foreign content. It recognizes retired approval
rows only after verifying both output files and the approval metadata were
removed for its exact owned attachments. Browser behavior is exercised through
[`drawing-upload.mjs`](../tests/browser/drawing-upload.mjs) and the
[native Edit tests](../tests/browser/native-drawing-edit.test.mjs).

Replay recording, validation, storage, delivery and playback still need full
integration. Ordinary `/i/` Draw and its replay controls remain disabled, and
the source's `javascript:oeReplay(...)` link is not generated. Complete source
parity still requires those replay paths. [Native replay data](native-replay-data.md) records
the existing replay format work.
