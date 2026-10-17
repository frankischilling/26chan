# Network request hints

The bounded hint evaluator is part of the unfinished network-admission work in
#212. Its browser and request signatures are legacy abuse-policy comparison
values. They do not establish identity, verify a browser, or authorize posting,
deletion or staff access. Public and staff posting pass these derived values
into posting. Network evaluation applies only when the source bypass is absent;
private accepted-post metadata still captures applicable hints for staff.

## Browser hint

The browser calculation retains the source's case-sensitive mobile markers,
exact `?1` hint, slash/colon numeric-version removal, and reduced Android user
agent model rule. Empty and exactly `0` user agents retain the source's falsey
marker. Header values are processed as bytes, without Unicode replacement or
silent truncation.

A new operator key supplies the legacy HMAC-SHA1 calculation. The source's
embedded deployment key is never reused. Public uses `BROWSER_HINT_KEY`; staff
uses `STAFF_BROWSER_HINT_KEY`. Each service rejects the other service's setting.
Key parsing requires a 32-byte key represented by 64 hexadecimal digits and
rejects an all-zero key. Errors do not echo the key. User agent and model inputs are bounded at 4,096 and 1,024 bytes, respectively.
Control bytes other than horizontal tab are refused. These bounds are deliberate
security limits.

## Request hint

The request calculation needs actual parser-order header names. It preserves
selected fetch-header order, client-hint grouping, required-header presence and
the source's fixed ordering of comparison flags. Case and underscore/hyphen
normalization match CGI/PHP keys; duplicate keys retain their first position.
The result is the first eight hexadecimal MD5 digits of those flags, as in the
source. It is not an integrity or authorization check.

The evaluator caps the input at 128 header names, each at most 128 bytes, and
requires valid HTTP token names. Public pseudo-headers cannot supply the order,
and `HeaderMap` iteration must not be used as a fallback. The opt-in
[Hyper extension](../third-party/hyper.md) obtains names from the existing
request parser without changing framing or keep-alive behavior.

## Posting integration

Both `/post` and the legacy `imgboard.php` post route derive hints from the
accepted request. The store receives only the derived strings, without raw
headers or key material. Duplicate normalized browser-value fields make the
browser provider unavailable because PHP/SAPI joining behavior is ambiguous.
Missing parser metadata makes the request provider unavailable. Neither case
borrows a value or ordering claim from a client-supplied pseudo-header.

The providers are independent: a definite match still applies if the other
provider is unavailable. A rule that needs unavailable evidence cannot silently
allow a post. Rules outside the request's scope do not require that evidence.
Authenticated staff bypass the network function before provider evaluation,
including unbadged posts and `/test/`. The outer content-filter condition alone
does not establish network applicability. See [staff network bypass](staff-network-bypass.md).

The actual TCP/HTTP/database test covers 11 combinations of routes, fetch-header
orders, spoofed order claims, duplicate browser headers and unavailable
providers. It checks accepted post IDs and verifies that refusals save neither
posts nor threads. These checks passed locally with the no-TLS test adapter;
full production and hosted qualification remain pending.

The earlier hint checkpoint passed 301 domain tests and 31 store/database cases.
Its staff rejection expectations omitted the authentication bypass and have been
replaced with authenticated-bypass and invalid-authority controls. At that
checkpoint, strict all-target/all-feature Clippy passed for
store, public and staff. Sixteen actual-binary startup cases cover invalid
keys, cross-role settings and valid-key controls that reach an occupied
listener without opening a database connection. The configuration role-isolation
tests also pass.

## Source evidence

`extract-network-hints-reference.php` selects only the two hint functions from
the hash-pinned supplied source. Before evaluation, it replaces the entire
embedded-key assignment with a fixed public synthetic key. It resets only the
per-request static caches between independent cases and handles the source's
specific absent-user-agent warning. It never loads the application, cookie
code, database, operator rules or deployment configuration.

The 1,168 executed cases cover falsey values, version stripping, mobile/model
conditions, ordered fetch headers, missing headers, normalized duplicates and
client-hint grouping. All cases and the bounded-input/key controls pass locally.
The fixture establishes function behavior for a supplied ordered server input;
it does not claim that every proxy or PHP SAPI produces the same input order.

    php scripts/extract-network-hints-reference.php SOURCE fixtures/network-hints-reference.json --check

The remaining network-admission work is tracked in #212. These fixtures do not
establish full feature or deployment parity.
