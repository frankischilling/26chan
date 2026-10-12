# Archive lifecycle comparison

This is the implementation and test map for issue #215. It compares the supplied
`imgboard.php` functions `trim_db`, `archive_thread`, `trim_archive` and
`rebuild_archive_list`, the archive JSON generator, and the imported board
settings. It does not claim that the original PHP application was run or that
the entire site has visual and behavioral parity.

## Capacity, transitions and reads

- Ordinary capacity excludes sticky and undead threads. Rollover uses oldest
  root activity when `EXPIRE_NEGLECTED` is enabled and otherwise post number.
  Staff-only boards skip active rollover. Board locks serialize competing posts
  and protection changes. `crates/store/tests/archives.rs` covers protected-only
  boards, overflow, ordering, queued flag changes and concurrent replies.
- The sticky/undead reply window retains the newest capacity-minus-one replies
  before inserting the accepted reply. Rejection rolls back pruning. See
  [reply retention](sticky-reply-retention.md) and its eight store tests.
- Ordinary autoarchive uses the configured surviving reply count, including the
  accepted final reply. It excludes protected threads. [Automatic archives](automatic-archives.md)
  maps store, HTTP, native-parser and browser coverage. Moderator force-archive
  has separate eligibility, reauthorization, rollback and audit tests in
  `apps/staff/tests/force_archive.rs`; see [staff force archive](staff-force-archive.md).
- Storage has no archive-count cap. Current board age applies to retained
  archives; enabled unlimited age is distinct from disabled archiving. A disabled
  board hides archives without deleting them. Policy changes cannot recover
  already-erased content. [Archive storage](archive-storage.md) covers policy
  locking, populated upgrades and dump/restore.
- HTML has a separate inclusive 72-hour window and 3,000-row ceiling. JSON IDs
  are ascending and complete within declared backend resource budgets. The
  source-shaped table and 100-scalar excerpt branches have separate rendering
  tests. Oversized reads fail instead of returning incomplete lists.
- Cleanup retires deadlines strictly before its database cutoff. Equality stays
  stored. The public view independently hides equality and older deadlines;
  unlike a static rebuild, readers need not wait for the next cleanup. The
  `archive_expiry_boundary` database test checks these two contracts separately.

`apps/public/tests/archive_storage_api.rs`, `archives.rs`, `archive_capacity.rs`
and `autoarchive_api.rs` exercise HTTP/API contracts. The archive and watcher
browser suites cover navigation, read-only state and native lifecycle behavior.
These test paths describe coverage, not the result of a particular CI run.

## Private data and source cleanup

The source clears host, email, password and Pass fields when archiving. The
rewrite does not persist a raw email/options field or a per-post Pass identifier.
Public Pass entitlement work remains in issue #201; this archive audit does not
implement that subsystem.

Existing archive triggers remove OP network addresses and own-reply membership,
per-post keyed posting history, and private poster-count contexts. See
[OP self bumps](source-op-bumps.md), [posting cooldowns](source-posting-cooldowns.md)
and [poster counts](private-poster-counts.md). Public poster labels use the
already-documented thread-scoped HMAC replacement; they do not reproduce the
original hash values. Archived JSON omits them.

Migration 0091 removes deletion hashes on new archive transitions. Migration
0139 also retires hashes left on older archives, including private boards,
disabled boards and deleted replies. Active credentials remain unchanged.
Anonymous proof derivatives can remain, but both proof functions require the
matching current deletion hash. The populated upgrade test verifies that they
no longer grant archived deletion authority and that active proofs still work.
See [archive credential retirement](archive-deletion-secrets.md).

The source removes every report kind on a post with fewer than three recorded
illegal-report contributions and keeps the group at three or more. Migration
0100 uses a private lifetime counter, rather than recounting surviving report
rows after a partial purge. Its tests cover the threshold, partial and complete
purges, bulk operations, rollback and real-role locking. Details are in
[report-group lifetimes](report-group-lifetimes.md).

## Deliberate retention boundaries

- Unknown or incomplete historical report counters are retained conservatively.
  Removed contributions cannot be reconstructed, so the migration does not
  guess a below-threshold count or sweep old archives.
- Report admission membership is separate from retained report records and
  moderation audit evidence. Archive retirement removes eligible membership;
  it does not erase that independent evidence or shared sessions.
- Cross-board action counters, shared cooldown/quota state and Robot9000 state
  retain their existing lifetimes. They do not restore retired archive ownership.
- Fresh whole-thread expiry and reply pruning clear payload and per-post author
  authority through migration 0129. Structural and media tombstones remain to
  prevent attachment capability reuse. Historical already-deleted payload,
  independent audit snapshots and physical media cleanup have separate rules;
  see [whole-post deletion](fresh-content-erasure.md).
- WAL, backups and client copies are outside live-row retirement. Restoring an
  older database requires pending migrations before reopening traffic. Actual
  deployment retention policy remains an operator responsibility.

These are backend privacy, integrity and evidence-retention differences, not
claims of byte-for-byte storage equivalence. They must remain visible when
assessing issue #215. Broader deletion policy (#214), Pass support (#201), full
page appearance (#193), and deployment qualification remain separate work.
