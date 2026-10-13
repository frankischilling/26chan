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
and Close controls. Zoom uses the source levels from 50% through 1600% without
changing the underlying image pixels. Speeds are 0.5, 1, 2, 5, 10 and 25 times normal; gapless
playback skips pauses longer than three seconds. Elapsed and total time use the
source formatter. Playback pauses when the page is hidden and resumes on return
only if it was playing before that automatic pause. Manual pause and rewind
clear the pending resume. Close, Escape and navigation release the worker and any pending
acquisition. A replay does not replace an active, unsaved painting.

The player fetches at most 262,400 canonical bytes, without credentials or
redirects. Its ten-second deadline includes stalled response reads. Each
worker command has a separate ten-second deadline and advances at most 50
source events. Only fixed, release-owned JavaScript can run in the worker;
it cannot fetch uploaded scripts or create another worker. Returned frames
are native ImageBitmaps and are closed after display or cancellation.

The read-only interface uses the pinned source UI builders in a separate shadow
root. Tool settings, color, layer selection, visibility and 24-pixel previews
come from a checked worker receipt. The receipt allows at most 25 layers and
contains no markup, URLs or source events. Uploaded recording bytes stay outside
the UI builders. The separate UI instance cannot replace the painter's retained
canvas or reuse its element IDs.

The full engine uses native OffscreenCanvas operations. Canvas and typed-array
allocation charges are conservative cumulative limits, not browser RSS
measurements. Rewind starts a fresh worker. Larger or unusually costly
recordings may hit these safety limits and show a playback error.

The media endpoint rechecks visibility, board recording policy and stored file
integrity on each request, including conditional requests. Deleted, hidden or
unapproved recordings cannot be fetched. Raw TGKR uploads are never served.

## Verification status

The source UI projection, elapsed timer and visibility-resume changes have local
controller and receipt-validation coverage. Their release-browser comparison
with the original UI is pending; the runs below describe earlier playback and
publication coverage.

The native drawing, source-renderer, worker-session and player suites passed at
commit `38e94e52a3c80b8815528b8754c61ba3791bb78e` in
[run 38359030629](https://github.com/frankischilling/26chan/actions/runs/38359030629).
The release-bundle checks caught a constructor-name minification error; the
build now preserves the names used by Tegaki's event table.

The real-guest publication test also exposed a fixture mistake: `commands.tgkr`
deliberately contains extreme signed coordinates for structural decoder tests.
Its decoded result must fail the publication envelope. That case now remains a
rejection control; `bounded-stroke.tgkr` supplies a separate positive recording,
packed by the pinned recorder with an independently transcribed canonical wire.
Neither synthetic recording claims pixel agreement with the red PNG fixture.

The real-guest coordinator and complete HTTP intake qualification steps passed
at the same commit in
[run 38359011401](https://github.com/frankischilling/26chan/actions/runs/38359011401).
The browser records trusted pointer strokes, submits the image and recording,
posts the approved attachment, compares playback pixels, edits the source post,
and checks that deletion revokes image and replay access. The coordinator cases
exercise joint publication and both parser and numeric-envelope refusals through
the actual mTLS gateway, broker and disposable guest.

Local PostgreSQL, intake, delivery, annotation and mTLS fixture tests also passed.
The local sandbox cannot create the Unix socket used by one public transport
test; that test remains enabled in hosted CI. These results describe the named
suites, not the outcome of every CI job. Merge still requires the complete check
matrix to pass on the final head.
