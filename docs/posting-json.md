# Posting JSON responses

Both `/{board}/post` and `/{board}/imgboard.php` accept the existing
URL-encoded posting form. Exactly one `Accept: application/json` header
selects a JSON response. Lists, parameters, different casing and duplicate
headers select the HTML path. This follows the supplied
`imgboard.php:3776,6816` exact-value branches; it is not general Accept
quality negotiation.

After the store commits a post, the response is HTTP 200 with numeric
`tid` and `pid` fields. A new thread returns `tid: 0`; a reply returns its
parent thread ID. `pid` is the inserted post ID. The source emits this shape
at `imgboard.php:6873-6879`. JSON success has no redirect, including when
the form requests `nonoko` or `nonokosage`. HTML success still uses the
rewrite's 303 redirect, not the source's timed success page.

The serializer emits each i64 as an exact decimal JSON number. Browser
clients must preserve that precision when reading the response; ordinary
`JSON.parse` can round identifiers above the safe-integer range. The
response does not add string aliases or change the source field types.

Posting-rule failures return HTTP 200 with only `{"error":"..."}`,
matching the source's `error_json` branch at `imgboard.php:3808-3816`.
Clients must check the error field, not just the HTTP status. Current
messages remain static rewrite messages and do not claim original wording.
The serializer escapes message text and never includes submitted passwords,
upload capabilities or database diagnostics.

Request-parser failures retain their 4xx status with a generic JSON error.
This includes unsupported content type, invalid typed fields and the
262,144-byte form limit. Infrastructure failures retain 5xx. Same-origin,
Fetch Metadata, rate, admission and timeout checks run outside the posting
handler and retain their existing status/body. These containment failures
must not be treated as source posting-rule failures. The separate read API
still rejects POST and does not expose the posting handler through CORS.

Posting responses have `Cache-Control: no-store` and `Vary: Accept`.
Successful JSON and HTML posts use the same optional, bounded, board-scoped
tracking and auto-watch receipts. Failures create no receipts. Receipt
creation follows the committed store operation, including approved image
posts; an already-consumed upload capability cannot create another post.
Receipts remain browser hints, not authorization.

[Issue #96](https://github.com/frankischilling/26chan/issues/96) tracks this
response slice. Unit tests check exact negotiation, integer serialization,
error encoding and actual router rejections with a closed lazy pool.
Database HTTP tests check both routes, persisted OP/reply IDs,
line normalization, HTML fallback, denied writes, closed threads, parser
limits, unavailable storage and attachment success/replay. A browser test
serves only an owned synthetic client document, submits to the real posting
routes, verifies saved content and receipts, then visits the real thread to
check tracking and watching. It deletes its own fixture through the public
password handler. That test does not change production CSP or implement a
Quick Reply interface. CI must pass the persisted/browser tests; a local
unit or compile pass alone does not qualify them.

The original multipart mode/field names, password-cookie flow, captcha,
full success page and Quick Reply interface remain separate work under
I-009 and E-010/E-011. Production CSP still permits no posting fetch from
ordinary pages. No schema, dependency, database grant or media-boundary
change is required by this response layer.
