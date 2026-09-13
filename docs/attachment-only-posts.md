# Attachment-only posting

Issue: https://github.com/frankischilling/26chan/issues/84

The public [thread API reference](https://github.com/4chan/4chan-API/blob/master/pages/Threads.md)
describes `com` as conditional on a supplied comment. Empty-comment attachments
are represented without `com`, including after file-only deletion. This is not
evidence that every native posting-validation detail has been reproduced.

## Request and form behavior

An empty or omitted `com` is accepted only when an attachment is requested and
the existing database authorization succeeds. Ordinary text-only posts still
require a non-whitespace comment. Nonempty whitespace-only comments remain
rejected even with an image; native whitespace normalization has not been
established. Name, subject, comment size, NUL, deletion-password, board, thread,
and image-limit checks remain in place.

The approved-upload HTML form makes its comment optional. Ordinary text forms
retain `required`. Neither path requires site JavaScript. Production media
remains disabled; this does not qualify or authorize a production deployment.

## Database boundary

Migration 0019 allows zero-length comments while preserving the 16,000-character
and 64,000-byte ceilings. A deferred constraint trigger rejects an empty row
without a durable `post_media` association at transaction completion. It also
rejects when the caller forces pending constraints immediately. The trigger
uses a fixed search path and the existing non-login attachment owner, with only
the additional comment-read permission. Runtime callers cannot execute the
trigger function, change comments, disable triggers, or insert/delete attachment
associations directly.

The existing scoped insertion function remains the only public attachment
writer. It checks capability, expiry after lock acquisition, published job,
approved output, one-use state, and board/thread limits before inserting both
rows. A later failure rolls back thread/reply changes, attachment consumption,
and deletion-secret insertion together. Request validation does not treat a
supplied capability as proof of approval.

The association survives file deletion, whole-post deletion, and queue
retention. Thus image-only posts can display a deleted-file marker without
restoring file access or reusing a capability. Trusted administrative cleanup
can still remove fixture/history rows; the constraint does not grant runtime
identities that authority.

## Regression coverage

- Domain tests distinguish empty, omitted-at-HTTP, whitespace-only, and bounded
  text, retaining the original text validator.
- Store authorization, expiry, unapproved output, one-use and image-limit races,
  rollback, moderation, cancellation, and retention cases run sequentially with
  both ordinary and empty attachment comments.
- Direct public-role insertion of an unattached empty row fails on commit or
  forced constraint evaluation; runtime permissions remain checked.
- HTTP tests cover empty image OPs, an omitted-comment image reply, invalid
  text-only submissions, optional form metadata, API omission, and deletion.
- The real no-JavaScript browser covers both text-plus-image and image-only
  posting locally. Native CI exercises image-only PNG and progressive JPEG
  through Firecracker while retaining text-plus-image baseline JPEG coverage.

Passing results must be recorded on the exact PR revision; this document names
the intended coverage and is not itself a qualification result.
