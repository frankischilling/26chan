# Global search compatibility

The public search page follows the pinned source page and client at `/globalsearch.php`. Its hash uses `#/query/board/page`, with `all` standing in for a global search when a page number is present. The browser shows 10 thread results per page and allows at most 10 pages.

The historical client called a separate `find` service whose implementation is not present in the supplied source tree. This rewrite keeps the same browser-facing result shape but serves it from the public application at `/search/api`. The query runs with the public database role and therefore inherits board and post row-level visibility rules.

To keep the reconstructed service bounded, each request searches the newest 20,000 visible posts, groups matches by thread, and returns at most 10 threads for the requested offset. A result contains the thread OP and at most five matching replies. Query hashes are limited to 512 UTF-16 code units, offsets are limited to 0 through 90 in steps of 10, and JSON output is capped at 1 MiB. Search compares the query case-insensitively against stored subjects and comments. These limits are part of the local service contract because the missing historical backend provides no source-backed resource policy to reproduce.
