# Multipart text posting

The public native form submits multipart data to `/{board}/imgboard.php`,
using `mode=regist` and `pwd`. The supplied `views/imgboard.php:45-52`
establishes that transport and those fields. The dispatcher at
`imgboard.php:10295-10300` also accepts `mode=post`. Both Rust posting
routes accept either multipart or URL-encoded bodies, with the same typed
post model, validation, transaction and [JSON response contract](posting-json.md).

`pwd` and the existing `password` name are aliases. Supplying both is an
error. `mode` may be omitted by existing clients; when present it must be
`regist` or `post`. A mode cannot dispatch deletion, reporting or staff
operations through the posting handler. Those handlers retain their own
routes and authorization.

The parser accepts name, options, subject, comment, parent ID, posting
receipts and approved upload capabilities. Source checkbox value `on` and
the existing `true` representation are accepted; empty/`false` are false.
`MAX_FILE_SIZE` and `hasjs` are accepted as bounded form hints, without
changing server limits or granting authority. `textonly=on` cannot be
combined with an approved attachment capability. It does not bypass board
admission rules. Original board-specific file requirements remain separate
compatibility work.

Browsers send an empty `upfile` part when no file is selected. The parser
accepts that empty part, but rejects a selected filename or any file bytes
with instructions to use isolated intake. Text parts cannot masquerade as
file parts. The parser requires valid UTF-8, rejects repeated/unknown fields
and uses the route's 262,144-byte streaming limit. Its fixed field allowlist
also bounds the number of retained names. A bounded internal URL encoding
shares the existing typed parser; it cannot expand beyond three times the
input byte ceiling. Multipart syntax/body failures retain 4xx responses,
including 413 for oversized input. JSON parser errors contain static text,
not submitted values or credentials.

Normal and approved-image native forms use the same multipart fields. The
visible deletion-password control remains an explicit security exception
to the source's hidden `pwd`/UserPwd identity flow. Existing Argon2 password
requirements, receipt limits, origin checks, Fetch Metadata, admission,
timeout and rate limits remain enforced. No public media decoding, CSP
expansion, database grant, schema migration or new dependency is involved.

Local parser tests exercise source fields, both posting modes, duplicate
aliases, invalid modes/IDs/UTF-8, unknown fields, file substitution and
malformed boundaries. A chunk-counted oversized stream verifies rejection
before the complete request is consumed; 64 bounded property cases cover
arbitrary text inside the permitted field structure.

Database tests run both encodings through both actual routes, checking
committed OP/reply IDs, comments, HTML fallback, closed-thread rejection,
origin enforcement, parser limits and unavailable storage. An approved
image reply uses multipart capabilities and `spoiler=on`. Browser tests
exercise actual `FormData` JSON posting and JavaScript-disabled native
submission, including line endings at the scalar limit, UTF-8 field byte
limits, errors and tracking/watch receipt consumption. Visual tests compare
the existing desktop/mobile and attachment baselines without replacement.
Persisted/browser behavior requires the hosted database suite; local parser
and fixture tests do not substitute for it.

[Issue #98](https://github.com/frankischilling/26chan/issues/98) tracks the
text-form transport portion of I-009/E-011. This change does
not implement the original single-request raw-file pipeline, UserPwd/Pass
identity, captcha, timed success page, full toggle-hidden form or Quick
Reply UI. File intake and publication remain the isolated E-010 workflow.
