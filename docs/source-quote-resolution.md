# Source quote target resolution

Public comments with source formatter profiles 104 through 111 or 120 through 127 resolve
numeric post references against the same repeatable-read snapshot as their
page. Stored comments and historical formatter profiles do not change.

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

Leading-zero spellings use a numeric lookup key and preserve their original
label and fragment. An OP destination also preserves the thread spelling:
`>>000123` can become `/g/thread/000123#p000123`; a reply in thread 100 uses
`/g/thread/100#p000123`. Same-thread references keep `#p000123`.

Post zero is a deadlink after page resolution because it cannot exist in the
Rust store. Outside resolved page representations it stays plain text, avoiding
an invalid `/post/0` link. The source allowlist still takes precedence over
absence: an unknown cross-board reference stays plain.

Source word wrapping runs before numeric matching. A generated break ends the
digit run, including a run of zeroes. The implementation does not impose a
19-character limit on leading-zero source labels; historical tokens retain
their previous limit. Cross-board terms admit an optional sign and lowercase
exponent, matching the source's post-wrap alphabet. The lookup uses PHP's 64-bit
string conversion: fractional values truncate, finite overflow saturates, and
infinite or underflowed values resolve to post-zero absence. Labels and hrefs
retain the original spelling. Uppercase E and decimal points are outside this
callback alphabet. A same-board digit-prefix token, such as the first part of >>123abc, remains a
normal lexical match; the trailing text is not part of its lookup key.

The browser validators preserve these numeric post fragments and local thread
routes. Source hover, inline quotes and mobile companion links recognize only
decimal fragments; signed and exponent links retain ordinary navigation. They preserve lexical spellings for DOM lookup and backlink
annotations. A leading-zero fragment therefore misses a canonical visible DOM
post and uses remote lookup, as the source does. The transport requests its fixed
same-origin endpoint with an exact canonical ID and omitted credentials.
Snapshot envelopes and DOM recipe identities remain canonical; a lexical alias
in a response cannot bypass validation.

Inline self and ancestor limits use numeric board/post identity so a leading-zero
alias cannot bypass those resource guards. That is a deliberate security
restriction: the source self-quote check compares DOM spelling. Deadlink spans are text-only and cannot
acquire link, preview, inline-quote or backlink authority. Prepared remote quote
copies bind fragments to their validated source thread rather than the viewing
page.

## Quote appearance

Resolved missing references use the source theme's quote color and line-through
decoration. The selector is scoped to comment deadlinks so watcher state styling
keeps its own rules. Ordinary comment links also follow the source's 480px mobile
color cascade: worksafe links use #34345c; non-worksafe links use #00e and quotes
use #000080. Mobile hover colors follow the selected board family. Dark mobile
links use #81a2be and hover at #5f89ac; non-link dead spans retain their theme color. The Never use
mobile layout preference preserves desktop colors. The six supplied stylesheet excerpts are pinned in
fixtures/source-quote-styles.json; the theme tests compare browser-computed
properties and retain desktop/mobile page screenshots. A second reference uses
full pages from the original renderer, with the same owned comments. See
[source link page comparison](source-link-page-comparison.md) for its inputs,
asset substitutions and review limits.

## Conditional responses

Body-derived ETags cover resolved output. A referenced post can disappear or
become visible without changing the referencing thread's timestamp. Responses
with these lookup dependencies therefore do not use that timestamp for
If-Modified-Since revalidation. Missing targets count as dependencies too.
Other representations retain their existing cache policy.

## Reference and qualification

fixtures/quote-resolution-reference.json contains hash-pinned source excerpts,
224 controlled target cases and five lexical cases. Controlled absence is a
presentation input, not evidence of the source database's visibility policy.
The optional PHP oracle executes the extracted lookup and rendering functions
with synthetic database stubs, including the source `%d` conversion. It does not
load the original application or contact a database.

fixtures/source-quote-numeric.json adds 53 explicit signed, exponent and overflow
cases, including the distinction between PHP comparison and SQL integer coercion.
The five entity cases in the base fixture check the exact captured ENT_QUOTES
statement. JSON comments use its named entity spellings after typed template
escaping; no entity is decoded and historical profiles retain their output.

fixtures/source-quote-spellings.json also captures seven exact functions from
the pinned original extension. Isolated synthetic contexts check lexical DOM
lookup, backlinks, remote JSON comparison, source self-quote behavior and
inert signed/exponent inline/mobile links without
starting the source application. The numeric JavaScript comparison cases remain
within safe integer precision; the rewrite retains exact signed i64 IDs.

The earlier leading-zero change passed 144 domain tests, the persisted public quote
resolution and conditional-response test, all 138 public library tests, strict
all-target/all-feature Clippy, and actual Chromium preview/backlink/inline
contracts. The isolated PHP oracle passes all 229 cases, and all 14 offline
Python reference tests passed. Those are historical results, not qualification
of later changes. Current-head results belong in the pull request; complete
hosted CI is required before merge.

Generated resources retain their 256 KiB filter and 32 KiB quote-feature limits.
Additional parse5 private methods are minified only after build checks establish
that no application property or quoted lookup uses those names. Exported APIs,
parser validation and resource limits remain unchanged.

This work does not settle every quote spelling, the missing historical database
collation, external search rendering, every RSS field or full rewrite parity.
