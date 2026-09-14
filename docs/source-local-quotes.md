# Same-board quote cleanup

Issue: https://github.com/frankischilling/26chan/issues/133

New comments follow `imgboard.php:5378`: an exact `>>>/board/` prefix followed by
an ASCII digit becomes `>>` when the target is the board receiving the post.
The source performs this global text rewrite after its early Unicode filters and
before spacing cleanup and private-codepoint removal. It is not a lookup of the
referenced post and does not depend on that post being present or visible.

The rewrite preserves digit bytes, including leading zeros, zero and values
beyond integer ranges. It applies inside spoiler/code-looking text and embedded
references. Other-board, differently cased, malformed and non-digit targets stay
unchanged at this stage. The existing typed formatter decides which resulting
tokens can render as links. Its integer bounds and markup support are separate
from this source text transformation.

The board is carried with the existing sanitation policy from the locked board
record, not taken from a client-supplied quote target. A literal bounded-prefix
scan performs the rewrite; user text cannot become a regex, path or SQL fragment.
The raw character/byte budget applies before shortening. No extra lookup,
runtime grant, schema migration, dependency or HTML trust bypass is introduced.

Ordering is observable. On a normal board, the earlier finite Unicode map can
turn fullwidth quote punctuation and digits into a reference that then rewrites.
The `/a/`, `/jp/` and SJIS exceptions retain unmapped digits. A private codepoint
removed only after the quote stage can expose a reference that remains in its
cross-board form, matching the source's single pass. Historical stored comments
are never rewritten on reads.

## Verification and deployment

Domain tests cover exact/global matching, embedded references, leading zeros,
oversized digit strings, malformed targets, source ordering, raw limits, 1,000
near matches and 128 generated literal cases. Database/HTTP tests cover both
aliases and encodings across all four spacing policies, exact stored text,
normal-versus-cross-board JSON/HTML links, spoiler text, historical retention and
raw-limit denial without changing thread clocks/counters. Approved-attachment
fixtures keep capability and reuse assertions while checking the same rewrite.
The no-JavaScript browser case navigates a rewritten local quote alongside an
unchanged cross-board quote and retains deletion/missing-target checks.

Apply after the parent Unicode cleanup and migration 0027. No new migration is
needed. Binary rollback affects future comments; it cannot restore an explicit
board prefix removed from already stored text. Backup/restore preserves stored
values without reprocessing.

Complete current-head hosted database/browser qualification remains required.
This does not complete source markup, name/trip sanitation or word filters.
[Subject sanitation](source-subject-cleanup.md) does not rewrite local quotes.
[Line admission](source-line-admission.md) implements intra-word spoiler removal
before local quotes, and repetition/line limits after spacing cleanup. Deployed
containment, recovery and independent review remain launch prerequisites.
