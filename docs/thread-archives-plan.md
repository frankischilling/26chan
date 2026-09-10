# Thread archive implementation plan

Goal: complete the documented archive lifecycle and HTTP path in
[the design](thread-archives-design.md), in the authorized current checkout on
`feature/thread-archives`. Use Rust 1.94 and existing locked dependencies, safe
first-party Rust, synthetic owned data and the configured Git identity.

1. Add a failing real-public-role rollover test in
   `crates/store/tests/archives.rs`: create a board limited to one thread, post
   twice, and require the second write to succeed while the first is hidden.
   Run `cargo test -p board-store --features database-tests --test archives --locked`.
2. Add migration 0009 and model fields. Implement `ArchiveEntry { id: i64,
   subject: String, archived_at: DateTime<Utc> }`, `ArchiveSnapshot { board: Board,
   entries: Vec<ArchiveEntry> }` and
   `archive_snapshot(pool: &PgPool, slug: &str) -> Result<ArchiveSnapshot, StoreError>`.
   Implement rollover under the existing board lock, expiry-aware reads and
   mutation guards, with no new public closed/sticky grant. Extend real tests for
   enabled archives, expiry, caps, ordering, grants and concurrent writes.
3. Implement public/API routes, archive fields, summaries and read-only pages in
   `apps/public`, updating synthetic visual constructors for new model fields.
   Write failing router assertions first, then verify both listeners with real
   database fixtures and a JavaScript-disabled browser flow.
4. Cover staff reopen/sticky denial and removal in `apps/staff`; test the actual
   SQL operation under the staff login. Add an owned migration-upgrade exercise
   from 0008 to 0009 and wire it into CI.
5. Record compatibility/operating policy, exact verification and known limits.
   Run formatting, warnings-denied workspace Clippy, locked builds/all-feature
   tests, public/staff browser checks, migration/restore and workflow checks.
   Obtain whole-change source review; fix findings and rerun affected checks.
6. Commit and push the actual implementation, open a focused PR linked to the
   issue, inspect final-head hosted CI, merge after passing checks, and verify
   the resulting main tree. PR status records final integration completion.

Execution: lifecycle/read paths are tightly coupled and will be implemented
manually; the public presentation task may be delegated once its store interface
exists. Review uses a separate read-only reviewer. No production deployment or
old private-source import is part of this change.
