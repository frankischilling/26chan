# Global search verification

The reconstructed search workflow for #195 follows `Search` in the supplied
`4chan-old/js/extension.js` and `rebuild_search_page` in `imgboard.php`.
The historical `find` backend is absent from that checkout. The replacement
service contract and its resource limits are documented in [public search](public-search.md).

On October 1, 2026, these checks passed against the local implementation:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p board-store --test search --features database-tests --locked` | One persisted search/visibility test passed in Ubuntu under WSL |
| `cargo test -p board-public --lib --all-features --locked` | All 99 library tests passed in Ubuntu under WSL |
| `npm run test:global-search` | Bundle matched at 182,200 bytes; six Node cases and four Windows Chromium cases passed |

Browser fixtures create and delete their own threads through real posting routes.
The tests exercise both viewports, the ten-result page size, direct hash links,
pagination, zero results, malformed hashes and API requests, escaped hostile text,
bounded excerpts and public visibility after deletion. Source searches replace
the current history entry; restoration tests change the hash explicitly rather
than expecting submissions to create history entries.

The first browser runs exposed fixture errors: an empty `resto` value, incorrect
API parameter names and an incorrect 400 expectation for domain validation,
which returns 422. Those expectations now follow the actual typed contracts.
No application assertions, retries or screenshot tolerances were disabled.

A WSL invocation of the npm command failed because this checkout's esbuild
installation contains Windows binaries. That invocation is not counted as a
pass. The Rust checks ran in WSL and the browser command ran on Windows.
Hosted checks must pass on the pushed head before merge. This checkpoint adds
no media containment or full rewrite completion claim.

## Hash-contract correction, October 8, 2026

The source-derived table exposed three gaps: the old check omitted `#/` from
its length limit, slashless hashes became queries, and decimal `parseInt`
differed from the source's bitwise page coercion. Those paths now match the
unchanged pinned `Search` methods from revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The fixture records the source file
hash, byte range and snippet hash. Node tests execute that snippet directly and
reject all three regression mutations. The table covers UTF-16 boundaries,
percent escapes, extra slash segments, unknown boards, fractions, hexadecimal
pages, infinities, negatives and 32-bit wrapping. Cancellation tests cover
cleared, over-limit and malformed replacement hashes.

Local checks passed: all nine Node search tests, browser-spec syntax, and the
regenerated bundle check (186,837 bytes). The two new browser scenarios navigate
the real search page and API at desktop and mobile widths. They have not run in
this environment: no usable local browser is available. Hosted CI must run all
six search browser scenarios before this correction is considered browser-verified.
No retries, screenshot tolerances or application assertions were relaxed.

## Response observation repair, October 8, 2026

Hosted run `37827983314` failed while reading the direct-hash response through
DevTools: Chromium reported that the response body was unavailable. The test
now captures a bounded clone in the page before the search client consumes and
cancels its response. It still checks the actual HTTP status, response offset,
one-request count, query, board, source-derived values and rendered fields.
The observer neither replaces the network response nor issues another request.

On the Windows checkout, all 11 search Node tests passed, including a real
Chromium request through the generated client followed by controller abort.
The observer also rejects invalid UTF-8, malformed JSON and oversized captures
while preserving the original response. After `npm ci --ignore-scripts`, all
generated-asset checks, browser-spec syntax and `git diff --check` passed.
The six database-backed page scenarios remain subject to fresh hosted CI.
