# Legacy deletion and report routes

`/{board}/imgboard.php` now accepts password-authorized deletion and opens a
report form for a visible post. Posting through this route continues to use the
existing transaction, field validation and HTML/JSON response selection.

## Public reference

The released extension v1191 supplies the request shapes. Its bytes and
collection date are pinned in [the watcher asset manifest](public-watcher-assets.json):
182,061 bytes, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
`Del.deletePost` sends `mode=usrdel`, a numeric post-name field whose value is
`delete`, and `onlyimgdel=on` for file-only deletion. `Report.open` opens a GET
request with `mode=report` and `no`. The extension recognizes `Updating index`
in a successful deletion response. No live external write or report was sent.

The reference does not establish the server's authorization, report categories,
captcha policy or complete success-page appearance. This implementation retains
the project's explicit deletion password and free-text report reason. Same-origin
routes replace the original separate posting domain. Archived deletion and bulk
deletion are outside this adapter's supported contract.

## Request and authorization rules

Deletion accepts either URL-encoded fields or bounded text-only multipart. It
requires one canonical positive signed-64-bit post ID and one `pwd` or `password`
field. IDs retain their exact decimal value, including values above JavaScript's
safe-integer range. `onlyimgdel=on` selects the existing file-only transaction.
Duplicate, conflicting, unknown or incomplete deletion fields are rejected.
An optional query `mode` must agree with a body `mode` when both are present.

Both encodings share the actual streamed 256 KiB request limit. Field decoding
stops after 20 entries. Multipart rejects nonempty file parts before they can
reach posting or deletion. The numeric field is enabled only on the legacy
adapter; the ordinary posting extractor keeps its narrower schema.

The existing middleware checks Origin and fetch metadata and applies request,
write-rate, connection and hash-operation limits. Deletion looks up the post
within its requested board and verifies the stored Argon2id password. The OP
ownership and deletion paths now share the same fixed-profile verifier, which
rejects unsupported stored parameters before running the hash. OP deletion
hides the thread; file-only deletion preserves text and revokes media access.

Successful legacy deletion returns bounded, escaped, script-free HTML with the
public client's success marker and a return link. The existing `/delete` route
keeps its redirect response. Neither route reflects the password.

The GET report route validates its query and visible board/post before rendering
an accessible form. Submission uses the existing `/report` transaction and staff
queue. Missing or removed posts return 404. The form requires no JavaScript and
inherits the normal CSP, framing and response-size policies.

## Verification

`legacy_form` tests cover both encodings, exact IDs, duplicate and conflicting
fields, unsupported modes, field-count overflow and streamed body overflow.
`legacy_report` rejects malformed queries before storage access. The
`legacy_deletion` database test creates posts, verifies persisted reports, checks
wrong-board/password/origin rejection and confirms deletion through the read API.
The existing attachment test also exercises legacy multipart file-only deletion,
including wrong-password rejection and revoked reader access.

`legacy-actions.spec.js` checks real posting/report/deletion with JavaScript
enabled and disabled. The JavaScript case sends actual browser FormData under
the application CSP and Origin checks. Each case removes its own synthetic post.

```text
cargo test -p board-public --all-features --locked
npm run test:display
```

The consolidated verification result is recorded in
[rewrite completion verification](verification-rewrite-completion.md). These
routes advance the public compatibility inventory in #6; they do not establish
the missing original report policy or production abuse controls.
