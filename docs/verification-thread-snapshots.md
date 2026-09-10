# Thread response snapshot consistency

On September 9, 2026 (September 10 UTC), an owned PostgreSQL regression
reproduced mixed thread responses on HTML and both JSON listeners. The handlers
read board settings before the existing thread transaction. A committed change
between those reads could combine an old board title or bump limit with newer
thread metadata and posts. This is tracked in [issue #26](https://github.com/frankischilling/26chan/issues/26),
separately from the [board-list snapshot work](verification-board-snapshots.md).

`board_store::thread_snapshot` now returns a `ThreadSnapshot` containing the
board, thread and visible posts from one `REPEATABLE READ, READ ONLY`
transaction. Both handlers use those settings, and the transaction commits
before rendering. The existing 1,001-post bound, ordering and deletion filters
are unchanged. The Rust return type changed; all repository callers were
updated. No public HTTP interface changed.

## Regression evidence

The test uses a generated board, the actual `board_public` database identity
and a separate owner connection. A fresh lazy pool with one connection pauses
its first release through SQLx's asynchronous `after_release` callback. A writer
then atomically changes board settings, metadata and post content. The original
handlers return that connection after reading the board; the corrected handlers
return it after completing the whole snapshot. The test requires the in-flight
body to equal either complete before or complete after state, and verifies those
controls differ. No production synchronization hook is added.
The callback records timeout failures explicitly, checked after pool closure;
SQLx discarding a connection cannot silently turn a broken barrier into a pass.

The initial test failed on all three routes. After the fix, it passed. Expanded
coverage uses a second barrier: an owned posts-table lock and PostgreSQL's actual
blocking-PID relation establish that the request has reached the blocked query
before the writer commits. This catches inconsistent queries even when a single
transaction contains them. Temporarily selecting `READ COMMITTED` made all three
table-lock cases fail; the exact original source bytes were restored in a
`finally` block before final verification.

All six barrier/route combinations now pass. Settled checks cover ordered
visible posts, retained lifetime reply counts after deletion, JSON field values,
body ETag invalidation after a board-only bump-limit change, empty 304 bodies,
missing/deleted thread 404s with the existing error text and `no-store`, and
invalid board names. The owned data and pools are cleaned even when an assertion
is caught by the outer fixture.

One new test initially expected `Not found.` instead of the existing
`Board, thread, or post not found.` message. Source inspection confirmed the
existing behavior and the assertion was corrected; the runtime error mapping
did not change. A private native helper initially passed a CR-terminated
`--locked` argument and was normalized to LF before any test ran. A sensitivity
script's initial occurrence-count check rejected its input before editing; its
corrected version produced the intended isolation failure above.

## Commands and results

Native verification used Rust 1.94.0, PostgreSQL 16.15 and the existing disposable
database credentials, sourced privately without rotation. From the repository
in the owned Ubuntu environment:

```sh
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/staff.env
export CARGO_HOME=/opt/26chan-rust/cargo
export RUSTUP_HOME=/opt/26chan-rust/rustup
export CARGO_TARGET_DIR=/opt/26chan-rust/target
export PATH=/opt/26chan-rust/cargo/bin:$PATH
cargo test -p board-public --features database-tests --test thread_snapshots --locked
cargo test -p board-public -p board-store --all-features --all-targets --locked
cargo clippy -p board-public -p board-store --all-features --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The focused regression passed after correction. The full public/store run passed
30 public tests and eight store tests, including the existing board snapshot,
permission, publication, queue and concurrency cases. Scoped Clippy, formatting
and whitespace checks passed. After the callback-timeout guard was added, the
focused regression passed again in 0.74 seconds. Fresh source review accepted
the runtime change, both barriers, teardown and evidence claims, including that
guard, with no reported findings. The reviewer did not execute tests. Hosted
checks remain separate from these local results and are tracked in the draft
PR linked from issue #26. CI already discovers the new database test through
its all-feature workspace run.

## Scope

This satisfies the project requirement that thread representations remain
consistent during concurrent commits. It adds no evidence for undocumented
original posting behavior or complete 1:1 compatibility. Templates, styles,
routes, JSON shapes, HTTP error mapping, cache policy, schema, grants and
dependencies are unchanged. Existing admission and handler deadlines still
apply. These tests establish neither a new memory budget nor a database query
cancellation deadline. Date-validator behavior is unchanged; the board-only
cache check specifically uses the body ETag. Production and public media
enablement remain disabled.
