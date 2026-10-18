# Hyper request-header order

`vendor/hyper` contains the official Hyper 1.11.1 crate with a small, opt-in
request-metadata patch. The downloaded crate archive was checked against the
workspace lockfile before extraction:

- SHA-256: `27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43`
- Upstream revision from the crate metadata: `6371cd425017155f7fbecef0e57b218edbe6a93a`
- The upstream license, package metadata and source remain in the vendor folder.

The source network heuristic depends on the order of selected request headers.
A Rust `HeaderMap` does not provide wire-order evidence. The patch records names
in Hyper's existing validated HTTP/1 parser loop and attaches a read-only
`RequestHeaderOrder` extension to each accepted request. It adds no second HTTP
parser and changes no framing, validation, response or keep-alive branch.

The `server-header-order` feature exposes the extension and
`http1::Builder::capture_request_header_order`. Collection defaults to false.
The application explicitly opts in through its public and staff connection
budgets; other listeners retain the default. Only names are retained, including
repeated names in arrival order. No values or raw request bytes are added to the
extension. Existing header-count and request-head byte limits bound storage.

The existing FFI-only order tracker is unchanged. The new feature needs neither
unstable FFI flags nor FFI behavior. Other than the feature and metadata
plumbing, changes to upstream parser test contexts only initialize the new
opt-in field to false.

## Qualification

The actual TCP transport test passes with collection both off and on. It checks
case-normalized duplicate names and two pipelined requests with different
orders, so a connection cannot reuse the previous request's order. All 14
TCP transport regression tests pass, including deadlines, cancellation,
keep-alive and connection-capacity recovery. Five local
feature combinations compile: no features, client HTTP/1, server HTTP/1,
server header order, and client/server header order.

The CI qualification script runs upstream library tests with client/server
HTTP/1 and HTTP/2, then repeats them with `server-header-order`. The main CI aggregate
requires that job to pass. The official locked test dependencies are now cached locally. Both upstream
runs pass 115 tests, with the same six upstream-ignored tests in each run.
Hosted qualification remains required.

The first hosted parser job failed because the selected upstream test feature
set omitted HTTP/2, which provides its shared `Compat` test adapter. The test
script now enables that feature in both comparison runs. Application HTTP/1
features and runtime behavior are unchanged. The corrected local suites pass;
the corrected hosted suite still needs to pass.

The application projects repeated names to the first normalized CGI/PHP key
position before computing the legacy hint. Proxy and SAPI behavior can affect
that order; selected-function fixtures establish the algorithm for a supplied
ordered input, not historical production ingress behavior. See
[network hints](../admission/network-hints.md).
