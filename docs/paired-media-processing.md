# Inactive paired candidate processing

This slice connects a persisted paired input to the versioned dispatcher and disposable guest. It returns an independently checked transport candidate. It does not admit a replay, approve an image, install output files, publish an asset, or enable a route, board, editor or viewer.

## Ownership

- `crates/store/src/paired_candidates.rs` and migration 0116 own the typed paired claim and terminal candidate receipt.
- `crates/media/src/paired_snapshot.rs` owns the immutable input snapshot and actual-byte descriptor checks.
- `apps/media-admin/src/paired.rs` owns candidate orchestration, PNG-component source provenance, fresh attempt binding and independent host output checking.
- `crates/media-dispatch` owns explicit Rust framing, mTLS exchange and gateway forwarding.
- `scripts/media/dispatch_protocol.py`, `dispatch-broker.py` and `run-job.py` own broker framing, selected-version disk construction and retained-inode collection.
- `apps/media-guest` owns independent input-disk framing, sequential PNG/TGKR decoding and fixed joint result encoding.

## Claim and completion

`MediaQueue::claim_paired_candidate` calls a restricted SQL function. The function locks the oldest eligible paired job, changes queued to processing, increments its attempt count and gives it a fresh 30-second lease. Generic image claim still excludes paired jobs. The existing paired descriptor trigger preserves input kind and the finalized byte/hash tuple. A new guard prevents the media role from creating or extending a paired processing lease with direct SQL.

Migration 0116 allows paired processing, but still forbids paired published state and every paired asset. It retains generic expiry/retry behavior and the three-attempt ceiling. The typed completion function locks the job and checks its kind, state, token and fresh expiry. A checked candidate ends in failed with the explicit `candidate_checked` reason, no lease and no output hash or length. This means transport-only qualification completed and its result was discarded from the production media pipeline. It is never a user-facing success or media-safety status. Rejected processing uses the existing processing_failed reason. Terminal jobs cannot be revived.

A future joint approval path must consume a still-current paired lease under separately qualified admission and all-file publication rules. It must not revive a terminal candidate_checked job. No CLI, production route or scheduler invokes this candidate API.

## Bytes and provenance

The coordinator opens the claimed generated quarantine object once, checks its regular-file length, then reads one bounded immutable snapshot with exact EOF. Parsing that snapshot computes the bundle and component hashes and compares every persisted descriptor. The same snapshot supplies the dispatch bytes and source provenance. No pathname is reopened for either purpose.

PNG provenance uses only the PNG component and its raw SHA-256/length. The bundle hash remains a separate field. TGKR parsing and image decoding remain inside the guest; host PNG source framing is the existing bounded digest routine.

Each invocation creates a new 32-byte nonce from operating-system randomness. The transmitted binding is SHA-256 of the versioned attempt domain, job ID, nonce and verified bundle hash. Neither the intake bearer nor processing lease token enters dispatch, guest disks, guest arguments or guest environment.

## Versioned transport

The v2 request is `IBJOB002`, big-endian u64 bundle length, 32-byte binding, exact completed `IBPAIR02` bundle, then EOF. The bundle range is 57 through 16,777,272 bytes, with separate 8,388,608-byte PNG and replay limits. The gateway and broker select an explicit outer protocol version and preserve it; they never infer kind from image bytes or retry through v1.

The guest input disk contains that complete request followed by zero padding to a 512-byte boundary. The trusted kernel argument is `board_media_input_kind=paired-v2`. Unknown or repeated arguments fail. A missing argument retains old image-v1 boot compatibility, without inspecting payload bytes. Init passes the selected mode explicitly to the worker.

The guest requires exact padded disk length and all-zero padding before decoding. It decodes PNG, then parses TGKR and encodes canonical IBRPLY01 wire. It writes the 64-byte IBRES002 header, compact IBRGBA01, optional IBRPLY01 and zero padding to exactly 4,456,960 bytes. PNG and replay dimensions are independently bounded; equal canvases and pixel correspondence are not required.

The v2 response is `IBOUT002`, big-endian u64 4,456,960, the exact result disk, then EOF. Version mismatch, truncation, extra bytes and missing clean TLS EOF fail. The host independently checks inner binding, presence, reserved bits, lengths, padding, RGBA dimensions and replay wire. No state or cost helper result is used as admission authority.

## Resources and remaining qualification

V1 input/output sizes are unchanged. V2 selects its own input disk and output allocation/collection size. Guest and jailer file-size allowances follow the selected fixed result size. CPU, address-space/RSS, process limits, cgroups, 96 MiB tmpfs, timeout, no-new-privileges, identities and process-tree cleanup remain unchanged. The runner retains the pre-opened output inode and collects it only after process termination.

Local protocol, source and SQL checks do not establish real Firecracker behavior, combined maximum-input resource viability, browser playback fidelity, replay admission or complete source parity. Actual guest qualification must rebuild the initramfs from this guest and test absent/present replay, independent dimensions, stale bindings and malformed/maximal envelopes under the existing runtime limits. Hosted gateway/broker tests need their root lane. Production activation and immutable joint publication remain separate work.
