# Text-thread transfer

This work covers one part of issue #222: moving a supported text-only thread to
another board and deleting its original posts. Media transfers, copying all
threads, and keeping or archiving the source after a move remain separate work.
The database and HTTP tests pass. Full regression and browser qualification are
still in progress.

## Source behavior

The reference is revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
`imgboard.php:1455–1732` implements the single-thread move. It reads the OP first,
then replies, allocates destination IDs and copies saved public fields. The reply
query has no `ORDER BY`; this implementation uses ascending reply ID so a move
has a repeatable result.

At line 1555 the source calls `str_replace` on the saved comment using mappings
from earlier successful inserts. It appends the current mapping at lines
1622–1623. Those are sequential literal replacements, including prefix matches
and cascades. An OP has no earlier mapping. A reply's own ID and later reply IDs
are not added early. For example, replacing `>>1` with `>>2`, then `>>2` with
`>>9`, changes `>>12` to `>>92`.

Saved comments already contain their formatting. Lines 5828–5843 normalize links,
wrap text, establish quote spans and remove temporary markers before the posting
insert at 6525–6529. The move inserts its transformed saved comment at 1571–1578.
Rendering calls `auto_link` at line 2032; its late quote resolver is at 4103–4120.
Moving a comment must preserve the earlier formatting while allowing those late
post references to resolve on the destination board.

`scripts/extract-thread-transfer-reference.py` verifies hashes of these source
excerpts and 22 manually transcribed literal-replacement cases. Its optional
`--php-oracle` runs only the captured assignment with synthetic strings. It does
not load the original application or its configuration. The Python cross-check
is not a substitute for the PHP run.

## Frozen presentation

`board_domain::saved_presentation` stores a finite sequence of typed text,
markup, line and word breaks, changed entities, and qualified static/server
links. FP01 uses ordinary bounds; FP02 uses the saved staff-authorized profile.
Neither representation grants posting or staff identity authority.

Admission accepts current saved formats 104–111 and 120–127. It checks the full
source projection for existing WF or FP input. Legacy formats, inconsistent
payloads, unsupported token types and over-limit inputs are refused before the
transfer can change content. Finalized links outside the codec's finite grammar
are also refused, including links whose marker cleanup leaves a spelling that
cannot round-trip through the decoder.

Ordered replacements operate only inside text runs. Markup, word breaks,
entities and fixed links retain their boundaries. Dynamic references are lexed
again after replacement; word filters, markup parsing, link normalization and
word wrapping are not rerun. Repeated moves decode the saved FP representation.

The shared rewrite budget reserves work before cloning, scanning, traversing
nested labels or constructing output. Failed expansion attempts consume their
scan budget, and exhausted reservations leave zero remaining. This measures
bounded logical work, not exact CPU time or allocator cost. An error leaves the
original presentation unchanged.

Frozen rows use separate `saved_presentation` and `saved_presentation_search`
columns. Both wordfilter columns must be NULL. Source wordfilter provenance stays
inside FP; it is not a new wordfilter program. Readers decode using the persisted
limits profile. Search may carry a shortened comment beside the complete typed
payload, so readers do not repeat admission's full-comment equality check.
Corrupt or conflicting payloads render as bounded escaped text with no markup or
quote parsing.

## Transfer authority

The browser sends only the source board/thread, destination, CSRF value and an
explicit delete-original selection. The server prepares a short-lived private
intent, transforms the exact prepared source, seals the output, then consumes
that intent in one content transaction. The consumer accepts only the ticket;
it has no argument for replacement text, public attribution or ID mappings.

The auth issuer is trusted to run the formatter. SQL binds its sealed output to
an immutable map and unchanged source snapshot; it does not independently prove
the formatter algorithm. The private map used during preparation cannot bypass
posting triggers. Only an internal, transaction-bound map created by the
validated consumer can qualify transferred inserts.

The limits are one pending intent per account, two globally, a fixed
60-second lifetime, a 128 MiB logical reservation budget and a shared 256 MiB
rewrite-work budget. Source and output representations each keep the existing
64,064,000-byte thread-read ceiling. These are backend resource limits and may
refuse a source-valid move. There is no truncation, partial destination or
automatic retry after refusal.

Account/session guards must be released before calling across the auth and staff
pools. Transfer entry points recheck authority under their own locks. Pending
records have no cascading foreign keys to live sessions or source posts. Revoked
work cannot be consumed and expires through bounded cleanup. Durable result
records allow the same account, with current access to both boards, to recover a
committed result after an interrupted response.

Readiness checks the qualified schema, privileges, function definitions and
trigger inventory. Its catalog fingerprint uses PostgreSQL's canonical constraint
deparse with fixed timestamp settings so administrator dump/restore remains
valid when object IDs and redundant expression parentheses change. Altered
constraints, privileges or trigger gates still fail readiness.

## Remaining qualification

The domain suite currently passes 246 tests, including 23 frozen-presentation
tests. Strict domain Clippy passes. Independent review identified and verified
repairs to failed-work budget accounting and final-link round trips. Read-side
qualification passes three frozen rendering tests and four quote-target tests.
Eighteen of the 22 literal source cases have qualified typed inputs; the four
remaining cases exercise source HTML spellings that this adapter does not admit.

The focused database and HTTP run passes 13 private-authority tests and three
staff-route tests. These cover preserved private evidence, credential changes,
forged authority, lock contention, rollback after a late failure and durable
result recovery. Populated migration, readiness drift and administrator
dump/restore checks pass, including changed caller timestamp settings. Fresh
role bootstrap and the existing deleted-report cleanup upgrade/restore checks
also pass.

The isolated PHP 8.4 oracle passes all 22 pinned replacement cases. The supported
local database-feature regression run passes 1,103 tests, with one ignored and
four Unix-socket tests excluded because this executor rejects those sockets.
All 42 affected staff rollback tests also pass with eight test threads. Their
faults use owned-board audit conflicts, so they still exercise late transaction
failure without changing the shared readiness catalog.

Browser assertions pass syntax validation but have not run locally because the
browser installation is unavailable. Hosted CI must still pass the browser and
Unix-socket checks, as well as every other required check on the exact PR head.
Issue #222 and the complete rewrite remain open.
