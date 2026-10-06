# Legacy post-lookup redirects

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
register activity or submit reports. Bare index redirects and other legacy GET
modes remain outside this change.

## Verification

Persisted HTTP tests use the actual public login and owned synthetic data. They
cover exact large IDs, source-shaped URLs, malicious Referer and identity cookies,
query rejection, visibility, retained archives, unchanged report handling and
before/after read-only snapshots. Browser tests follow real live and archived
OP/reply redirect chains with JavaScript enabled and disabled. Browser execution
requires exact-head CI; these contracts do not claim a locally run browser.
