# File-only deletion actions

Migration 0130 follows `imgboard.php:2545–2582,2613–2667` for live file-only
actions. A successful request sets a post-level deletion marker and refreshes
thread modification time, including repeated requests and text-only posts.
Existing attachment deletion flags remain effective. The migration adds a
false-default column without rewriting historical flags or creating attachment
records for text-only posts.

Public requests retain the existing password/session proof, eligibility,
revocation checks and per-peer quota. A successful repeated or text-only action
charges its batch just like another successful deletion. Public file-only actions
preserve reports and post text.

An administrative staff action removes reports for the exact
target, including resolved, dismissed and historical reports, within the same
transaction as the file marker, thread timestamp and audit record. Cascades remove
private membership, anonymous ownership and weight evidence. Target report rows
carrying clear history are removed too. Unrelated report history and independent
moderation audit records remain. A failed authorization or audit write rolls back the whole action.

Migration 0140 adds [staff self-deletion](staff-self-deletion.md) using a verified
posting peer or recovery password, following `imgboard.php:2490–2497,2584–2594`.
Those file-only requests preserve reports and use a distinct audit action.
Account authorship does not substitute for either proof. See
[deletion parity](deletion-parity.md) for the full acceptance coverage.

## Visibility and authority

JSON and native updater snapshots expose the effective post-or-attachment marker,
including on text-only posts (`json.php:380–406`). Thread and preview HTML require
an actual attachment before rendering a file block, matching
`imgboard.php:2044,2125`; text-only markers do not fabricate a deleted-file block.
Catalogs use the deleted-file placeholder as in `catalog.php:177`. Media reads
reject assets linked to marked posts. Consumed upload receipts and durable media
identities remain reserved.

The file-only store action locks board, thread and post in that order, then
rechecks visibility and archive expiry after waiting. Report admission uses the same board lock. Private cleanup runs through the existing NOLOGIN owner; runtime roles gain
neither report DELETE nor access to private report evidence. Public and staff
readiness checks verify the marker, helper ownership and privileges, cleanup
binding and media projections.

Sticky OP staff actions use `imgboard.php:2509–2512`: an ordinary sticky OP
requires Admin. The source archived-deletion exception still requires Moderator,
but migration 0009 forbids persisted threads that are both archived and sticky.
That exception has pure policy unit coverage only. HTTP tests cover live sticky
OPs, replies in sticky threads and nonsticky archived OPs. File-only actions
retain archive visibility and expiry checks; sticky replies retain their existing
policy.

Whole deletion still uses migration 0129's canonical erasure. The new marker is
reset to false during that transition, and erased rows remain immutable.

## Migration and verification

Stop writers, apply the forward migration, then start matching public and staff
binaries. Older binaries do not implement the post-level marker semantics.
Rollback cannot recover reports removed by successful administrative actions;
use the established backup and restore procedure when recovery is required.

Regression tests cover repeat and text-only actions, public report preservation,
administrative target isolation, private evidence cascades, rollback, report
admission ordering, quota charging, sticky authority, rendering and readiness
faults. Exact executed commands and results belong to the PR checkpoint; this
inventory does not claim that qualification has finished.

Local qualification on PostgreSQL 16 passed 287 store tests, 10 public HTTP
integration tests, 148 public unit tests, 27 staff HTTP integration tests and 49
staff unit tests. One public Unix-socket test was filtered because the local
execution environment rejects that socket operation. The staff HTTP matrix uses
44 valid persisted combinations; the source archived-sticky exception is covered
separately by the pure policy test.

The populated migration and administrator restore checks for 0130, 0122 and 0129
passed with local toolchain adapters; their SQL assertions were unchanged. Rust
formatting and strict Clippy passed for every target and feature in the store,
public and staff packages. Hosted CI, including native environments, must still
pass on the exact PR head before merge.

The full-schema preservation fixture includes migration 0187's explicit warning
capacity singleton (`warning_limit = 100000`). Its warning table must remain
empty during file deletion. The baseline adds only that known row; it does not
recapture live tables. Rolled-back controls change the capacity and insert a
synthetic warning, and both must fail the retained-state assertion. This corrects
the earlier fixture omission without permitting unrelated state changes.
