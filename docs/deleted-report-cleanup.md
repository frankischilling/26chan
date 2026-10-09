# Reports after whole-post deletion

The supplied `4chan-old/imgboard.php:2720–2733` removes reports for a deleted
reply, or for the OP and every reply when the thread is deleted. Migration 0122
applies that behavior to fresh whole-deletion transitions in the rewrite.

## Transaction and authority

The existing post and thread deletion triggers call a private, fixed-search-path
function owned by the existing NOLOGIN report-admission owner. It deletes report
rows matching the actual board and post/thread relationships, including resolved,
dismissed, cleared and historical reports without private membership. Foreign-key
cascades remove membership, anonymous report activity and weight evidence. The
existing membership trigger retires empty group lifetimes. Independent moderation
audit rows are unchanged.

The function keeps the existing Read Committed and nonwaiting board-lock checks.
Normal mutation paths already hold that lock. Report admission also takes it, so
new reports cannot survive a committed whole deletion through a concurrent
admission. If deletion or its later authorization/audit step rolls back, report
cleanup rolls back with it.

Only the private owner gains report DELETE authority. Public and staff runtime
roles cannot call the private trigger function or directly delete, truncate or
install triggers on the report table. Readiness checks require the new trigger
bindings, restricted privileges and evidence cascades.

## Boundaries

- Public and staff file-only deletion keep their existing behavior.
- Source staff-self versus other-person file deletion remains unresolved.
- The migration does not sweep reports whose targets were already deleted.
  The staff queue and resolve/dismiss lookup exclude these targets. Historical
  physical cleanup needs a separate bounded operator procedure.
- Migration 0122 alone retains post text. Migration 0123 adds
  [fresh content erasure](fresh-content-erasure.md) while preserving structural
  media tombstones. Historical retention and issue #214 remain open.
- Archive transitions that do not delete the thread retain their separate
  report-group retirement rules. Actual archive expiration is whole deletion.

Apply the forward migration with writers stopped before starting matching public
and staff binaries. A binary rollback cannot recover reports removed after the
migration. Use the established backup and restoration procedure when recovery
of an earlier database state is required.

## Verification

The regression tests cover whole-reply/thread scope, unrelated records,
historical report shapes, evidence cascades, runtime permissions, rollback and
concurrent reporting. Staff tests exercise queue visibility and stale actions.
Exact executed commands and outcomes are recorded in the PR; this test inventory
is not a claim that every check has passed.
