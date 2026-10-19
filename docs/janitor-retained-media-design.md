# Retained request media

Draft implementation for the remaining media work in issue #223. Database
lifecycle checks pass on owned fixtures; HTTP and browser qualification are
still in progress. This work is not ready to merge.

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

## Development transport

The staff process reads approved representations from a numeric loopback HTTP
media origin. DNS names, credentials in URLs, remote addresses, redirects and
encoded responses are refused. Each response must match the planned MIME type,
length and digest. The client has a two-second connection timeout and an
eight-second total deadline; cancellation closes its socket. It never sends a
staff cookie or authorization header to the media service.

The transport accepts the existing approved-file ceilings: 10 MiB for PNG/JPEG,
20 MiB for GIF, 8 MiB for PDF/WebM/MP4, and 5 MiB for thumbnails. The database
rechecks the actual representation metadata before retaining bytes. Custody has
a 256 MiB default budget and a 1 GiB configuration ceiling; exhaustion refuses
the request without deleting its public post.

`migrations/0214_janitor_request_media.sql` keeps report classification behind a
private helper owned by the report authority. The restriction owner receives a
private approved-metadata view; it gains no direct access to additional report
or attachment columns. Public media credentials cannot read custody tables.
