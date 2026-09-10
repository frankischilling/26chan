# Public board response snapshots

Issue [#22](https://github.com/frankischilling/26chan/issues/22) identified responses assembled from different committed database states. Board settings, thread metadata, previews and visible counts previously used separate statements through the pool. A concurrent commit could therefore produce a response matching neither the complete old state nor the complete new state.

`board_store::board_snapshot` now reads these values in one `REPEATABLE READ, READ ONLY` transaction and commits before rendering. PostgreSQL 16 documents that successive reads in this isolation level use the same snapshot; transaction characteristics must be set before the first data statement. See the [isolation documentation](https://www.postgresql.org/docs/16/transaction-iso.html#XACT-REPEATABLE-READ) and [SET TRANSACTION](https://www.postgresql.org/docs/16/sql-set-transaction.html), inspected September 9, 2026.

The operation validates page and preview bounds, selects at most the board's 1,000-thread schema limit, aggregates visible counts in SQL, and optionally fetches the OP plus at most five latest replies per selected thread. Count-only lists fetch no comment bodies. Public/API index and catalog JSON use five replies; HTML board pages use three and HTML catalogs use the OP only. Ordering, page grouping, fields, omission rules and representation-derived ETags remain unchanged. This advances compatibility entries I-003, I-004, I-005, I-007 and I-008 without establishing original visual or interactive parity.

## Deterministic concurrent-commit regression

The test uses a uniquely named synthetic board, the actual `board_public` login and a separate disposable migration connection for fixture ownership. The writer locks the posts table. After the reader is proven blocked by that specific writer through PostgreSQL's blocking-PID inventory, the writer commits related board, thread and post changes. No production test hook, arbitrary commit delay or mocked database response is used.

For each route, the test compares the response obtained during that commit with complete responses before and after it. It must match one complete state. At baseline `eb06867`, all eight update cases failed this assertion: catalog, thread-list and index JSON on both listeners, plus HTML board and catalog. Fixture rows and connections were cleaned before propagating the captured test failure. After implementation, the same regression passed. Coverage then added thread deletion at the same controlled boundary, for sixteen concurrent cases in total.

The same owned fixture verifies sticky/bump/id ordering, two-page grouping, count-only reads, OP and latest-reply limits, deleted-reply exclusion, visible versus lifetime reply counts, omission fields, invalid page/preview extremes, settled ETag invalidation and 304 responses, whole-thread deletion and empty board/catalog/index responses. Assertions run in a captured task so ordinary assertion failures still release the owned fixture and connections. The table lock is only suitable for the isolated test database; no live service was exercised.

## Local validation

Checks ran on September 9, 2026, with the existing Rust 1.94.0 and PostgreSQL 16.15 environment. Native commands used `CARGO_HOME=/opt/26chan-rust/cargo`, `RUSTUP_HOME=/opt/26chan-rust/rustup` and `CARGO_TARGET_DIR=/opt/26chan-rust/target`. The existing `.local/database.env`, `.local/media.env`, `.local/media-reader.env` and `.local/staff.env` were sourced privately as required; their values were neither changed nor logged.

| Command | Result |
|---|---|
| `cargo test -p board-public --test board_snapshots --features database-tests --locked -- --nocapture` | Initial regression failed on all eight routes; corrected version passed, then expanded sixteen-case/settled coverage passed |
| `cargo test -p board-store -p board-public --all-features --locked` | Passed: 29 public and 8 store tests, zero failures or ignored tests |
| `cargo clippy -p board-store -p board-public --all-targets --all-features --locked -- -D warnings` | Passed |

Formatting, Windows browser checks, independent source review and hosted checks are remaining checkpoint gates at this documentation draft.

## Scope

No schema, database grant, registry dependency, public route, template or stylesheet changed. The snapshot holds a connection only during its bounded data selection; rendering and response-body retention happen afterward. Existing ten-second handler deadlines and response admission remain in force. These tests do not establish a new aggregate memory budget or server-side database cancellation deadline. Single-thread board-setting consistency is outside this board-response fix. Media dispatch and production qualification remain separate work; this branch enables no uploads, merges, releases or deployments.
