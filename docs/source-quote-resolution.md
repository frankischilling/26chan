# Source quote target resolution

Public comments with source formatter profiles 104 through 111 or 120 through 127 resolve
canonical positive decimal post references against the same repeatable-read
snapshot as their page. Parsing and stored comments do not change.

The pinned source callbacks in imgboard.php:4139-4217 determine presentation:

- A missing target is a deadlink span with its original escaped label.
- A same-thread local reference uses a post fragment.
- A board-index, other-thread or cross-board reference uses the target thread
  and post fragment.
- A cross-board reference outside the source allowlist stays plain text.
- References from mlp to b or co are deadlinks. The local implementation
  skips their target lookup; the source checks the target before this restriction.

The allowlist is separate from header navigation and the installed board
inventory. The local application uses root-relative routes instead of the
source's external board host.

## Lookup and visibility

The store collects typed references after the saved formatter and authorized
wordfilter limits have been applied. It deduplicates paired board/post keys and
uses one parameterized lookup inside the existing read transaction. It never
fetches target bodies, titles, attachments or secrets.

Public row-level security remains active. Explicit checks also exclude private
boards, deleted posts and inaccessible threads. A reply needs a live, visible
opening post before it can supply a public destination. That extra visibility
guard is backend hardening, not a claim about the source callback's SQL.

A snapshot admits at most 16,384 distinct lookup keys. Exceeding this resource
limit fails the read before the target lookup SQL; it does not silently render excess targets as
missing. Forced deadlinks and disallowed boards do not consume lookup capacity.
Metadata-only reads do not look up post targets.

## Rendering boundaries

Resolution applies to board and thread HTML, full archived threads, full/tail
thread JSON, board/catalog JSON, native updater and board fragments, and
single-post quote previews. Both JSON listeners share the same projection.

Catalog search text and archive/catalog excerpts keep their existing
pre-resolution representation. Search excerpts retain their existing rendering
and are not qualified by this slice. [RSS descriptions](rss-feeds.md) retain
source-profile post numbers as saved text, without target lookup. Staff previews
remain inert.

Historical formatter profiles keep their previous behavior. Leading-zero
tokens and unsupported cross-board numeric spellings retain the existing
unqualified path. No MySQL signed, exponent or nondecimal coercion is inferred.
A same-board digit-prefix token, such as the first part of >>123abc, remains a
normal lexical match; the trailing text is not part of its lookup key.

The browser validators admit only canonical, contextual post fragments and
the intended local thread routes. Deadlink spans are text-only and cannot
acquire link, preview, inline-quote or backlink authority. Prepared remote quote
copies bind fragments to their validated source thread rather than the viewing
page.

## Conditional responses

Body-derived ETags cover resolved output. A referenced post can disappear or
become visible without changing the referencing thread's timestamp. Responses
with these lookup dependencies therefore do not use that timestamp for
If-Modified-Since revalidation. Missing targets count as dependencies too.
Other representations retain their existing cache policy.

## Reference and qualification

fixtures/quote-resolution-reference.json contains hash-pinned source excerpts,
108 controlled target cases and five lexical cases. Controlled absence is a
presentation input, not evidence of the source database's visibility policy.
The optional PHP oracle executes only extracted functions with synthetic lookup
stubs; it does not load the original application or contact a database.

Local checks pass: 141 domain tests, the paired target visibility and
repeatable-read database test, two lookup-cap tests, ten public HTTP/snapshot
tests, and 136 public library tests. One Unix-socket transport test is explicitly
excluded because this environment cannot create Unix sockets. All-target
checks, Clippy with warnings denied, generated-asset checks and the 13 Python
reference tests also pass.

The client changes pass 77 focused Node tests. The generated filter remains
within its unchanged 256 KiB limit. Chromium and PHP are unavailable locally,
so browser execution and the extracted PHP oracle still need hosted CI.
Test discovery is not browser execution.

This work does not settle every quote spelling, the missing historical database
collation, external search rendering, every RSS field or full rewrite parity.
