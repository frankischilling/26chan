# Retained request media

Implementation plan for the remaining media work in issue #223. These paths are
not enabled yet.

## Source behavior

`admin.php:adminBanReq` copies the attachment and thumbnail only when the chosen
template has `save_post=everything` and no retained category 2 report exists for
the post. Other policies retain the post metadata without the file. The request
then removes the public post.

`ReportQueue.php:clear_ban_request` removes the full retained attachment when a
request is cleared. Denial also removes its thumbnail. Approval preserves the
thumbnail for the resulting ban. Amending to a `json_only` template removes the
retained media. Related requests cleared by the same review need the same
lifecycle handling.

The request client limits thumbnail display to a 130-pixel box without enlarging
small images. Its preferences hide thumbnails or expand them on hover, and the
I shortcut expands the focused request's attachment. Video and document handling
must follow the approved native representation rather than execute uploaded
active content.

## Storage and transaction boundary

Use private, quota-accounted database storage for the approved attachment and
thumbnail. This avoids making removed evidence physically readable by the
public media service. The public output store must not become a private archive.
Only already-approved representations may enter custody; no raw upload fallback
or decoder runs in the staff process.

Before deletion, a scoped plan identifies the expected representation, byte
limits and digests. The staff process fetches those representations from its
fixed media endpoint with bounded reads and no cookies, credentials or redirects.
The final writer rechecks session, board access, post and template state,
category 2 evidence, approval and byte digests under the existing mutation locks.
It stores the bytes and captures/deletes the post in one transaction. A stale
plan, missing representation or capacity refusal leaves the public post intact.

Queue rows contain only availability metadata and protected URLs. Reading a
retained representation requires a current session and access to that request's
board on every request. Responses use no-store, no referrer and strict MIME and
embedding restrictions. Conditional requests cannot bypass authorization.

Review removes full images and refunds their capacity atomically. Denial removes
thumbnails too; approval can move the thumbnail into private ban evidence. No
public route gains access to those bytes. Metadata-only policies and category 2
reports must never cause a media fetch or retain supplied bytes.

## Qualification required before enabling

- Source-derived policy cases for creation, denial, approval, amendments and
  related-request clearing.
- Real PostgreSQL rollback, digest mismatch, quota exhaustion, concurrent
  reviewers, category changes, revoked sessions and wrong-board controls.
- Private-table and function privilege checks, plus populated restore coverage.
- Bounded transport tests for truncation, redirects, unexpected MIME, wrong
  digests, timeouts and cancellation.
- Browser tests for hidden thumbnails, hover and keyboard expansion, navigation,
  repeated actions and script-disabled review.
- Public deletion remains effective while scoped staff reads work; after review,
  obsolete request media URLs refuse access.

This does not complete linked Pass history, staff-history search or full visual
parity. README showcase work remains reserved for verified overall completion.
