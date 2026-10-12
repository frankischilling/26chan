# Deletion behavior and security differences

Issue #214 covers author deletion, staff-self deletion, batch controls, feedback,
public erasure and private retention. The reference is the supplied original
`imgboard.php`, especially `delete_post`, `user_delete`, `resredir` and the action
thresholds, plus `views/imgboard.php`, `js/core.js` and `js/extension.js`.

## Acceptance coverage

- Authorization and timing: [public deletion eligibility](source-public-deletion.md)
  defines password/session authority, board switches, protected OPs, archived
  posts, network lifetime, exact minimum/maximum ages and error precedence.
  Domain boundary tests, `deletion_authorization` and `public_deletion_quota`
  exercise those rules, revocation and lock waits.
- Batch and UI behavior: [legacy actions](legacy-actions.md) and
  [full-page controls](source-deletion-controls.md) define ordered selection,
  partial commits, field budgets, file-only actions and thread/index return paths.
  `legacy_batch_deletion`, `legacy_deletion`, `source_post_actions`, native-client
  tests and the JavaScript-on/off browser scenarios cover these paths.
- State after deletion: [file-only actions](file-deletion-actions.md),
  [staff-self deletion](staff-self-deletion.md) and
  [report cleanup](deleted-report-cleanup.md) cover report differences, repeated
  actions, text-only targets, thread counts and clocks. `fresh_deletion_erasure`
  checks rendered pages, API output, search, media access and consumed-upload
  replay; staff tests cover administrative versus self-authorized actions.
- Erasure: [fresh content erasure](fresh-content-erasure.md) removes post payload
  and per-post author evidence transactionally. [Historical cleanup](historical-deletion-cleanup.md)
  handles previously retained rows through reviewed bounded receipts.
  [Deleted media metadata](deleted-media-metadata.md) clears descriptive attachment
  fields while preserving consumed identities. Tests include rollback, races,
  irreversibility, visibility/privilege drift and administrator restore.
- Independent private history: [moderation snapshot retention](moderation-snapshot-retention.md)
  defines the operator-selected 30-day period, exact expiry boundary, dedicated
  worker, bounded batches and restore procedure. Audit event metadata survives;
  saved content does not remain indefinitely. Report-time captures follow their
  target reports' deletion lifecycle instead.

These are test contracts. Passing one suite is not evidence that the entire
application or every browser passed. The pull request records final-head local
results and hosted CI, and remains draft until all issue criteria are qualified.

## Deliberate security differences

A shared peer address alone cannot authorize public deletion. The rewrite uses
opaque session ownership and hashed recovery passwords; trusted peer matching is
confined to authenticated staff-self classification. Request quotas charge the
first successful batch mutation even when a later selection fails, closing the
source's partial-failure accounting bypass.

Return navigation uses relative local URLs and private, non-cacheable responses.
It does not trust a Referer-derived scheme or shared-cache authenticated feedback.
Malformed and duplicate fields are rejected before any mutation.

The source physically removes post rows. The rewrite retains structural IDs and
consumed attachment associations to prevent media resurrection or capability
reuse. Their descriptive payload is erased. Shared sessions, abuse counters and
Robot9000 history retain their existing security lifecycles rather than
being reset by deletion. Global polls have no per-post ownership relationship.

The original private log is not a reason to retain deleted text forever. Existing
independent content snapshots expire after 30 days; deletion itself need not
create another copy of the removed content. Historical cleanup and backup
restoration are explicit operator procedures. None of these guarantees erasure
from WAL, backups, external logs or copies already received by clients.
