# Paired drawing intake

This slice implements completed PNG/replay intake and immutable input provenance. It does not activate public routes, drawing controls, `/i/`, guest processing, replay readers, or publication. Production intake still registers only v1. A database-test-only router exposes the new handlers for qualification.

## Browser contract

A future drawing handler can use `paired_upload::from_request` and the existing bearer reservation model. The body is ordinary `FormData`, in this exact order:

1. `resto`: canonical decimal nonnegative thread number, within signed 64-bit range.
2. `png_bytes`: canonical decimal PNG blob size, from 1 through 8,388,608.
3. `replay_bytes`: canonical decimal raw replay size, from 0 through 8,388,608. Zero means absent replay.
4. `upfile`: a blob named `tegaki.png`.
5. `replay`: a blob named `tegaki.tgkr`, present only when `replay_bytes` is positive.

The client does not supply a digest, path, asset ID, or per-part `Content-Length`. PNG and replay use separate budgets. The pair envelope is at most 16,777,272 bytes, including its 48-byte header and 8-byte completion marker. HTTP metadata has a separate bounded allowance. Replay cannot borrow the PNG allowance, nor can the PNG borrow replay space.

The parser accepts browser-style quoted `Content-Disposition` fields, optional PNG/octet-stream content types for file fields, and a boundary of 1 through 70 supported ASCII characters. Each part's headers are limited to 1024 bytes. Numeric values have at most 20 digits. Unknown, duplicate, or reordered fields and headers fail. Filenames are fixed transport labels, never paths.

Unlike a general MIME reader, the parser uses exact declared component lengths and requires the following delimiter at that position. It rejects preambles and epilogues. The final boundary may end at body EOF or have one final CRLF. No other bytes are accepted. Errors, cancellation, and missing EOF fail under one ten-second deadline covering declaration and forwarding, including intervening waits.

The public parser copies chunks of at most 16 KiB into a one-slot forwarding channel. It does not drain immediately-ready HTTP chunks into a whole-body buffer. Its request adapter bounds total HTTP frame bytes before `StreamReader` retains a frame; the current transport frame remains owned by that adapter until consumed. Intake uses an 8 KiB component buffer. Neither component is fully buffered or decoded by this path.

## Completion and provenance

Multer 3.1.0's `poll_next_field` changes its state to `Eof` after the closing boundary, without requiring the underlying stream to reach EOF. Its `StreamBuffer::poll_stream` also drains immediately-ready chunks into an internal buffer. Therefore the new path does not use `next_field() == None` as proof of HTTP completion. Existing v1 parsing is unchanged.

After the complete outer body has been accepted, the public forwarder emits `IBDONE02`. Intake separately validates the reserved job ID, pair header, component limits and lengths, exact marker, and EOF. It hashes the bytes actually read, syncs the private object, installs it without overwrite, and syncs the containing directory. Only then does SQL receive the total, PNG, and optional raw-replay length/SHA-256 tuple.

These hashes establish input identity, not image validity, replay safety, visual equality, or faithful guest execution. Image and replay canvas dimensions are not compared.

Reservation fixes the input kind. Pair descriptors can be set only during authorized finalization and remain immutable. SQL rejects v1 begin/finalize operations on pair jobs, paired processing/publication, and asset creation for pair jobs. The normal queue claim skips pairs. This prevents existing image-only/manual paths from granting approval while later slices are unfinished.

A failure before installation removes the receiver's own partial file. After installation, an uncertain SQL result preserves the completed object. The inactive `paired::reconcile` entry point authenticates the same bearer, opens and rehashes the completed object once, and retries the same finalization tuple. It never reuploads or replaces a component. Exact retry cannot change descriptors or revive an expired/failed reservation.

The receipt remains a one-use bearer attachment capability. This work adds no actor, board, or thread binding, second replay capability, or separate replay lifetime.

## Migration and service compatibility

Apply migration 0115 before starting the updated intake binary. It preserves
existing image-v1 rows and does not fabricate digests or handles. New pair
reservations remain inaccessible through production HTTP routes.

The intake role's function and column allowlists expand deliberately. Older
intake binaries reject that expanded catalog at startup, so upgrade the intake
binary together with the migration. A binary-only rollback to the older intake
release is not supported on the upgraded schema. Preserve provenance when
planning recovery; do not drop the new columns to make an old binary start.
The populated upgrade and transactional rollback are tested before commit.

Bearer cancellation revokes the attachment handle. It does not remove a queued
job; the existing abort operation remains receiving-only. Expired paired jobs
become failed and cannot be requeued, even with their original descriptor.

## Qualification and remaining work

The tests cover independent maximum components, arithmetic overflow, immutable actual-byte hashes, malformed and truncated envelopes, marker-without-EOF, split HTTP bodies, late body errors, cancellation, epilogue rejection, and standard FormData output. Separate disposable PostgreSQL tests exercise migration rollback, populated upgrades, restricted roles, descriptor shape, immutable transitions, exact retry, and legacy publication bypasses.

The 16,777,272-byte v2 aggregate replaces the earlier candidate-only shared 8 MiB policy. V1's 8 MiB limit is unchanged. Future dispatch, snapshot, disk, queue/storage accounting, and guest qualification must adopt the v2 limit explicitly. This slice does not increase guest CPU, memory, or deadlines.

Next work must implement the versioned isolated guest flow, qualify the complete replay consumer/admission policy, extend the immutable joint manifest and lifecycle, then wire readers and source-compatible drawing metadata. Current replay state/cost success is not publication authority. Full source/frontend parity and `/i/` activation remain unfinished.
