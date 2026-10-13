# Drawing replay integration

Issue #207 adds recording to the existing Tegaki drawing workflow. The editor
still uses the pinned 0.9.4 source. Replay processing accepts its full 28-event
vocabulary, including the bucket and pipette, pressure dynamics and layer
history. Imported post images keep recording disabled, as in the source.

## Upload and post

Drawing-enabled boards expose ordinary Draw and Quick Reply controls. A
recording-enabled board also gets a Replay checkbox, checked by default. Finish
exports one PNG and, when recording is active, one TGKR file. Export failure
keeps the drawing available for another attempt; it never silently drops a
recording and sends only the image.

The public drawing route checks board and thread policy before reservation.
Recorded drawings stream their PNG and recording together through a bounded multipart
parser into an explicitly enrolled drawing job. Image-only drawings retain the existing PNG upload path. The isolated decoder validates
both components. The coordinator independently checks the returned envelope,
canonical recording, dimensions, source identity and fresh attempt binding.
Only a complete image, thumbnail and optional canonical recording can be
approved together. Post attachment capabilities remain single-use.

Submitted drawing time keeps the source's wall-clock semantics; it is not an
authenticated measure of work. The existing post-write path resolves source
post references. The replay link comes from approved attachment metadata,
never a submitted URL or filename. A saved source reference takes precedence
over the replay link.

## Playback

The View link opens a player with Play/Pause, Rewind, Slower, Faster, Gapless
and Close controls. Speeds are 0.5, 1, 2, 5, 10 and 25 times normal; gapless
playback skips pauses longer than three seconds. Playback pauses when the page
is hidden. Close, Escape and navigation release the worker and any pending
acquisition. A replay does not replace an active, unsaved painting.

The player fetches at most 262,400 canonical bytes, without credentials or
redirects. Its ten-second deadline includes stalled response reads. Each
worker command has a separate ten-second deadline and advances at most 50
source events. Only fixed, release-owned JavaScript can run in the worker;
it cannot fetch uploaded scripts or create another worker. Returned frames
are native ImageBitmaps and are closed after display or cancellation.

The full engine uses native OffscreenCanvas operations. Canvas and typed-array
allocation charges are conservative cumulative limits, not browser RSS
measurements. Rewind starts a fresh worker. Larger or unusually costly
recordings may hit these safety limits and show a playback error.

The media endpoint rechecks visibility, board recording policy and stored file
integrity on each request, including conditional requests. Deleted, hidden or
unapproved recordings cannot be fetched. Raw TGKR uploads are never served.

## Verification status

The existing 41 native browser checks and 66 source-renderer, worker-session
and player checks passed at commit `d10c5f7c141f90197174b0c4611260ea6749fbee` in
[run 38349796710](https://github.com/frankischilling/26chan/actions/runs/38349796710).
The release-bundle checks caught a constructor-name minification error; the
build now preserves the names used by Tegaki's event table.

The complete drawing-post path and real-guest publication extension still need
final hosted qualification. Local focused PostgreSQL, intake, delivery,
annotation and mTLS fixture tests passed. The new real-guest coordinator cases
compile and pass strict Clippy; that does not establish guest runtime success. The local
sandbox cannot create the Unix socket used by one public transport test;
that test remains enabled in hosted CI. No merge or issue closure is implied
by these intermediate results.
