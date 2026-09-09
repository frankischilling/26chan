# Media quarantine implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the independent storage task and review. The lead handles database and operating integration in the current checkout as requested.

**Goal:** Persist bounded media intake and job lifecycle while keeping unverified processing disabled.

**Architecture:** A media library owns private storage and the narrow output protocol. PostgreSQL persists queue state under an independently restricted login. No decoder or arbitrary process launcher enters a credential-bearing application.

**Tech stack:** Existing Rust 1.94, Tokio, SQLx 0.9 and PostgreSQL 16; pinned PNG encoder and temporary-file dependency.

**Spec:** [media-design.md](media-design.md)

## Global constraints

- Keep `#![forbid(unsafe_code)]` in every new first-party Rust entry point.
- Use the requested current directory and `feature/media-quarantine` branch. Protect unrelated work and use the installed Git identity helper for commits.
- Public processing remains disabled; there is no unsandboxed fallback.
- All test fixtures are synthetic. Tests of the library or database are not microVM containment evidence.

## Task 1: Bounded private storage and output promotion

Files: create `crates/media/Cargo.toml`, `crates/media/src/{lib,id,quarantine,output,promotion}.rs`, and focused tests. Add the workspace member and locked dependencies. Do not modify the store, public application or documentation owned by the lead.

Interfaces: export `ObjectId` with random generation, strict lowercase 32-hex parsing and Display; `Quarantine` constructed with a trusted root; asynchronous bounded `receive` accepting an ID and `AsyncRead`; typed `ValidatedOutput` obtained only by reading the specified pixel protocol; and `Promoter` accepting only IDs and validated output. Supply filesystem paths only through trusted construction, never worker metadata. Give the lead exact final signatures before integrating.

- [x] Write failing behavioral tests for streaming at the limit and one byte over; empty/failed/cancelled input leaves no partial file; invalid IDs cannot select paths; duplicate intake never overwrites. Run them and record the missing-behavior failure.
- [x] Implement intake with an 8 MiB maximum and fixed-size read buffers. Clean temporary files on failure/cancellation. Expose bounded cleanup/removal by typed ID for queue reconciliation.
- [x] Write failing tests for zero/excessive dimensions, invalid magic, truncation and trailing bytes, plus a 1x1 known RGBA fixture. Implement the 16-byte header and bounded pixel reader. Restrict any output allocation to the independently computed dimensions.
- [x] Encode only validated pixels to PNG with pinned `png = "=0.18.1"`. Bound the resulting bytes independently. Test generated PNG pixels with a test-only decoder.
- [x] Test atomic no-clobber promotion, identical replay and conflicting replay, failure leaves no public partial file, and quarantine cannot also be the public root. Implement the smallest API satisfying those properties.
- [x] Run bounded property tests for arbitrary headers and IDs, `cargo test -p board-media --locked`, and `cargo clippy -p board-media --all-targets --locked -- -D warnings`. Commit only owned files through the Git workflow helper and provide an exact verification report.

## Task 2: Persistent admission, leases and operator intake

Files: create `migrations/0002_media_jobs.sql`, `crates/store/src/media.rs`, `crates/store/tests/media_queue.rs`, a small operator media application, and `scripts/dev-media-db.sh`; adjust workspace/development CI and restore scripts.

Interfaces: `reserve` returns an opaque ID; `queue` finalizes a completed intake; `claim` returns a job with attempt and lease token; token-bound `fail`/`complete` reject stale or expired ownership; cleanup returns bounded batches of expired/terminal objects. All SQL is bound and schema-qualified. Store tests use generated isolated queue namespaces only if needed for concurrency without changing production rules.

- [x] Write a failing real-role test requiring the new schema and login. Apply a versioned migration that exposes no media grants to public/staff/auth identities. Provision a disposable media login outside application runtime.
- [x] Exercise admission races, duplicate claims, failed intake, stale token reuse, expired leases, retry exhaustion and permission denials. Implement transaction/constraint enforcement and run the tests.
- [x] Integrate library intake with a development-only command, queue admission first, status and bounded terminal cleanup. Reject inherited privileged credentials and invalid configuration before opening input or storage.
- [x] Exercise the command on a harmless file against real PostgreSQL, including overflow and cleanup. Add a fixture-only integration test connecting claim, validation, publication and persisted completion. Never add a runnable unsandboxed worker.
- [x] Extend CI and disposable restoration to cover media tables and role separation. Run migrated workspace tests, formatter, clippy, builds, existing browser tests and dependency checks.

## Task 3: Review and evidence

Files: update architecture, compatibility, readiness, dependency, operations and verification documents; add focused GitHub issue and draft PR.

- [x] Record existing reference provenance unchanged and add project requirement IDs for implemented intake/control/protocol. Keep public uploads and worker containment marked disabled/unverified.
- [x] Document each new identity, storage scope, byte/state limit, command, cleanup and restore result. Record exact failures and unrun infrastructure checks.
- [x] Request an independent code review of the branch; fix substantive findings and rerun affected checks. Commit actual progress, push the authorized checkpoint and open a draft PR. Do not merge or deploy.
