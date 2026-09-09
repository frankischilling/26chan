# Durable Media Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Publish validated output through durable, lease-fenced approval records and recover interrupted publication without exposing pending files.

**Architecture:** A database reservation names one PNG per lease. One local storage lock covers reservation, atomic file installation, approval and cleanup. A separate database reader sees only approved metadata.

**Tech Stack:** Safe Rust 1.94.0, PostgreSQL 16, existing SQLx/Tokio/png/sha2/tempfile dependencies; PowerShell and WSL development scripts.

**Spec:** `docs/media-approval.md`

## Global Constraints

- Work on `feature/media-approval`, based on `5087d2ec520792bda49b8581b2b41dd658c7a9ee`. Do not read `4chan-old`.
- No production enablement, merges, new dependency versions, raw-file publication or privileged worker execution from a database-credentialed process.
- Use installed humanizer, git-commit-author and git-human-workflow skills. All Git/GitHub commands use `C:/Users/imike/.codex/skills/git-human-workflow/scripts/git-human-workflow.ps1`.
- Existing verified identity is Francis Hagan <frankhagan890@gmail.com>; do not change global identity.
- Preserve unknown/user files. Do not print credentials. Do not dispatch subagents from a worker.
- Database integration tests require the idle disposable local cluster. Coordinate with the controller before running them; no parallel queue integration suites.
- First-party Rust forbids unsafe code. Query user values through bound parameters. Preserve existing tests and APIs unless the plan explicitly changes them.

### Task 1: Durable approval database and reader role

**Files:** Create `migrations/0008_media_assets.sql`, `crates/store/src/media_assets.rs`, `crates/store/tests/media_assets.rs`, `scripts/dev-media-reader-db.sh`; modify `crates/store/src/lib.rs` and only the visibility needed in `crates/store/src/media.rs`.

**Interfaces:** Export `board_store::media_assets::{Asset, OutputMetadata, MediaReader}`. `Asset` has public `id: String, sha256: String, bytes: i64, width: i32, height: i32`, derives Clone/Debug/PartialEq/Eq/FromRow. `OutputMetadata` has the same metadata fields without id. Implement on `MediaQueue`:

```rust
pub async fn prepare_output(&self, job_id: &str, token: &str, metadata: &OutputMetadata) -> Result<Asset, StoreError>;
pub async fn approve_output(&self, job_id: &str, token: &str, output_id: &str) -> Result<Asset, StoreError>;
pub async fn output_cleanup_candidates(&self) -> Result<Vec<String>, StoreError>;
pub async fn begin_output_deletion(&self, output_id: &str) -> Result<bool, StoreError>;
pub async fn forget_output(&self, output_id: &str) -> Result<bool, StoreError>;
impl MediaReader {
    pub async fn connect(url: &str) -> Result<Self, StoreError>;
    pub async fn get(&self, output_id: &str) -> Result<Asset, StoreError>;
}
```

- [ ] Add failing integration coverage, run to capture RED, then implement the schema/API. Require exact lowercase hex IDs/tokens (32), digest (64), bytes 1..=5242880, dimensions 1..=1024 in both Rust validation and constraints. The table `media.assets` has independent random id, job_id, lease_token, immutable metadata, state pending/approved/deleting and timestamps; unique(job_id, lease_token), no cascading job FK. Use an approval view `media.approved_assets` with security_barrier=true exposing only Asset columns. A trigger disallows updating/deleting approved records for board_media while allowing owner-controlled fixture cleanup; disallow changing identity/metadata of a reservation. Grant the writer required table operations and the reader only view SELECT and schema USAGE.
- [ ] `prepare_output` locks job before asset, rechecks processing/current token/unexpired in the statement that inserts/reuses a pending reservation. Same metadata may replay approved state including after job metadata cleanup; changed metadata or deleting state must fail. `approve_output` locks job then asset; exact job/token/id required; updates job receipt only while current and unexpired, and asset approved in the same transaction. Same already-approved record is idempotent even after job cleanup. Legacy `complete` never creates an approval.
- [ ] Cleanup candidates are bounded 64, ordered, include deleting records and pending records without a matching live processing lease. `begin_output_deletion` first looks up job id, locks job if present, then asset, and rechecks eligibility in SQL using clock_timestamp; return false for live or approved rows. `forget_output` deletes only deleting rows. Document that all these operations require the publication storage lock held by caller. No output metadata or lease tokens in errors/logs.
- [ ] Reader connect checks exact board_media_read, no super/create/replication/bypass/memberships/database ownership; no content/post_secrets/staff_identity/deployment schema usage; no jobs/assets SELECT or writes, and no view writes. Require approval view SELECT. Keep pool private. Test valid reader, wrong role refusal, pending/deleting invisibility, approved positive read, writer/public/auth/staff denials as applicable, writes/base table/jobs/token denial with actual credentials.
- [ ] Cover duplicate prepare/approve, metadata mismatch, distinct IDs after lease turnover, stale completion before and after turnover, cleanup eligibility and idempotence, concurrent duplicate approval, approved survival after aged terminal job cleanup and DB immutability. Do not just assert SQL strings. Fixture deletion must target owned IDs and work after test panic. Existing env files `.local/database.ps1`, `.local/media.ps1`, `.local/staff.ps1`; new `.local/media-reader.ps1` created by bootstrap. Coordinate database execution with controller.
- [ ] Bootstrap script mirrors dev-media-db.sh's root/disposable-cluster guards, refuses existing credential files, creates board_media_read with random password and safe timeouts/search_path, grants only database CONNECT. Writes private ignored `.local/media-reader.env`/`.ps1` containing MEDIA_READ_DATABASE_URL. Execute only on authorized disposable development cluster after controller agreement, then run board-migrate with existing owner credential to apply migration.
- [ ] Run focused tests and formatting; commit only task-owned files through the Git wrapper. Report exact red/green commands, results, migration/grants and concerns in task-1-report.md.

### Task 2: Locked storage and operator publication workflow

**Files:** Create `crates/media/src/publication.rs`, `crates/media/tests/publication.rs`, `apps/media-admin/src/lib.rs`, `apps/media-admin/src/bin/media-publish.rs`, `apps/media-admin/src/bin/media-read.rs`, `apps/media-admin/tests/approval.rs`; modify media exports/output/promotion, media-admin manifest, config and configuration tests as needed. Controller executes this task alongside Task 1 without overlapping files.

**Interfaces:** Consume Task 1 API exactly. Expose host-encoded `EncodedOutput` from ValidatedOutput with metadata getters and private bytes. `PublicationStore::new(root, quarantine)`, `try_lock() -> PublicationGuard`, guard `install(id, &EncodedOutput)` and `remove(id)` operate only generated fixed id.png/id.part filenames. `ApprovedFiles::open(root)` and `read(id, sha256, bytes)` bound reads and verify SHA-256. Reader opens storage read-only and creates nothing.

- [ ] Write failing storage tests for lock contention/release, identical replay, conflicting bytes, fixed staging crash cleanup, immutable complete links, root overlap, digest/length failure and symlink/nonregular rejection. Capture RED then implement nonblocking permanent lock, bounded encoded output, sync before approval, no-clobber hard links, Unix directory sync, fixed staging file replacement only while locked. Reader never decodes. Reuse shared encoding/installation primitives with existing Promoter where safe.
- [ ] Add library `publish(queue, store, job_id, token, output)` that encodes validated pixels, acquires the storage lock, prepares the reservation, installs bytes, checks receipt metadata, then approves. Failure leaves pending private bytes. `reconcile(queue, store)` holds lock, drains one <=64 batch via begin deletion/remove/forget. `read_approved(reader, files, id)` must fetch approved metadata before filesystem read.
- [ ] Operator development CLI `media-publish claim LEASE_FILE`, `media-publish publish LEASE_FILE OUTPUT_DISK PRIVATE_STORE`, `media-publish reconcile PRIVATE_STORE`. Claim creates a new private operator file (Unix 0600; Windows current user's directory ACL required) containing job_id and lease_token, never stdout/token arguments. Publication reads at most 512 manifest bytes, denies unknown fields/invalid IDs, accepts regular stopped output disk of the exact bounded disk size, validates under a timeout, then uses the library. Existing MediaAdminSettings must reject inherited reader credentials too. Commands require explicit development mode. Generic errors omit secrets. No root runner invocation.
- [ ] Operator `media-read OUTPUT_ID PRIVATE_STORE DESTINATION` uses only MEDIA_READ_DATABASE_URL, requires explicit development mode and rejects inherited writer/public/staff/owner credentials. It opens the destination with create_new only after approval and digest checks and writes bounded bytes; no raw directory serving. Add config validation using existing test subprocess approach.
- [ ] Integration tests use actual roles and tempfile roots: approval gating before/after installation, expiration before approval, crash window pending staging/final and deleting record/file, no approved cleanup, durable approval after job removal, missing/corrupt file rejection, duplicate publication, lock prevents concurrent cleanup/publication. Add subprocess claim/publish/read checks. Commit controller-owned files after focused tests.

### Task 3: CI, recovery evidence and review

**Files:** Modify `.github/workflows/ci.yml`, `scripts/verify.sh`, migration/restore exercises, browser subprocess environment allowlists, docs/media-approval.md and relevant media/readiness/compatibility docs.

- [ ] Add reader bootstrap before migration, reader env source for integration verification only, remove reader credential from web child environments, require reader URL in verify script. Historic migration and restore exercises must include grants for the new role and compare durable approval data with a real isolated fixture; validate reader view positives and underlying-table denials after restore.
- [ ] Run full formatting, clippy all targets/features locked with -D warnings, workspace all-feature tests and browser suites. Run a stopped actual VM PNG through claim/manifest/publication/reader using the development operator workflow; report operator-mediated handoff honestly. Preserve public media-disabled readiness status.
- [ ] Have task and whole-branch reviews inspect changes without duplicating completed tests. Fix material findings with focused regression coverage. Update evidence with exact commands/platforms/counts and limitations. Commit/push and create a draft PR based on fix/media-orphan-recovery linked to issue #5; await hosted checks and record their actual outcome. No merge or production deployment.
