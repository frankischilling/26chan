# Legacy post lookups and index navigation

`GET /{board}/imgboard.php?res={post_id}` resolves a visible OP or reply to its
canonical thread and post anchor. It returns HTTP 301 with a relative Location:
`/{board}/thread/{thread_id}#p{post_id}`. The modern `/post/{id}` quote route keeps
its existing response contract.

## Reference and scope

The supplied `admin.php:1880–1884` generates this legacy link shape.
`imgboard.php::resredir` at 6883–6936 looks up the selected post and chooses the
OP itself or its parent thread. The default GET dispatch calls it for `res`.
Both successful and missing public targets receive a two-second cache lifetime.

The [source extractor](../scripts/extract-legacy-res-redirect-reference.py) pins
`imgboard.php` and `global_config.ini`. Its [20 cases](../apps/public/tests/fixtures/res-redirect.json)
exercise selected protocol and target/header/status snippets with recorders:
16 OP/reply redirects and four missing targets. Large identities remain exact
strings through signed-64-bit maximum. It does not execute database lookup,
input coercion, deletion, authorization or the application bootstrap.

The original derives an absolute HTTP/HTTPS destination from Referer. The
rewrite deliberately uses a local relative destination, so hostile or absent
Referer values cannot redirect to another origin or downgrade transport.

## Validation and visibility

The accepted lookup shape has one `res` field and a canonical positive signed
64-bit value. Duplicates, unknown fields, signs, leading zeroes and overflow are
rejected with 400. The accepted raw lookup query is limited to 128 bytes; this
is not a separate allocation bound for the framework's earlier query decoding.

The narrow store query returns only a thread ID. It does not fetch comment
bodies or resolve an anonymous session. Public visibility excludes private
boards, deleted posts/threads and expired archives. Missing or hidden targets
return the same 404 without a parent Location; retained visible archives work.
Both 301 and 404 use `Cache-Control: public, max-age=2`.

Explicit `mode=report` retains the existing report handler and its malformed
query shell. Mixed report/res fields are rejected there rather than producing
a redirect. Res-only lookups never acquire report popup privileges, mint cookies,
register activity or submit reports. Other legacy GET modes remain outside
these two finite branches.

## Bare index navigation

With no query or an exactly empty query, the same endpoint returns the source's
200 “Updating index...” page. It contains a two-second meta refresh to the local
`/{board}/` index and no Location header. This is navigation feedback, not an
index-rebuild operation. The source admin page links this destination under its
“Rebuild Index” label, but the selected default branch performs no rebuild.

The page validates public board visibility before rendering through the shared
encoded-output budget. Missing/private boards stay unavailable. A nonempty query
such as `&`, `mode=`, `res=` or `res=0` is not treated as bare and keeps its normal
rejection. HEAD retains the successful headers with an empty body.

The source's Times/serif 36pt centered, full-width, 300px table layout lives in a
same-origin stylesheet rather than requiring inline-style CSP permission. The
rewrite uses `private, no-store` and a local target; the source function sets no
cache header itself and derives an absolute scheme from Referer. Those are
explicit infrastructure/security differences.

The [index extractor](../scripts/extract-legacy-index-reference.py) runs only the
pinned `updating_index` echo/protocol function with synthetic inputs. Its
[eight cases](../apps/public/tests/fixtures/legacy-index.json) record exact body,
message, delay and style. They do not dynamically qualify HTTP status/cache,
authorization, endpoint dispatch, database behavior or rebuilding.

## Verification

Persisted HTTP tests use the actual public login and owned synthetic data. They
cover exact large IDs, source-shaped URLs, malicious Referer and identity cookies,
query rejection, visibility, retained archives, unchanged report handling and
before/after read-only snapshots. Bare-page tests also check HEAD and exact-byte,
short-buffer and retained-response output limits. Browser tests follow real live and archived
OP/reply redirect chains and native index meta refresh with JavaScript enabled
and disabled. Browser execution
requires exact-head CI; these contracts do not claim a locally run browser.
