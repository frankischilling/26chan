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
