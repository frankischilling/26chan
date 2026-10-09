# Source page headings and browser titles

Board names remain editable text. Visible board headings add `/slug/ - `,
except `/s4s/`, which uses `[s4s] - `. Index, thread, catalog and archive pages
share that heading. An archive list keeps the board name in its visible heading.

Browser titles follow the source page context:

- The first index page uses the board heading.
- Later index pages append ` - Page N`, with one-based page numbers.
- Catalog and archive lists append ` - Catalog` and ` - Archive`.
- Threads use the board prefix, OP context and short board name. Archived
  threads retain the same thread title.

Each title ends with ` - 4chan`, matching the reference. The page number comes
from the route; navigation links and display text do not grant authority.

## Thread context

The saved subject takes precedence. On upload boards, a leading decimal number
and pipe are removed first. The source's internal `SPOILER<>` subject marker
is removed only from source-escaped input. A user-submitted literal marker
remains ordinary text.

An empty subject falls back to the saved formatted comment. The projection
uses the persisted formatting profile, word-filter payload and generated
randomizer text. Source `<br>` tags become spaces; supported special entities
are decoded once; tags are removed; then the first 50 Unicode scalar values
are retained. Whitespace is not trimmed. SJIS blocks use the source's `[SJIS]`
replacement when that board policy is enabled. An empty result falls back to
`No.<thread ID>`. Private-board context follows the source's board-title branch.

All results are plain strings that Askama escapes. Captured source markup is
never rendered as trusted HTML. Hostile subjects and board names cannot close
the title or insert heading elements.

## Evidence and limits

The pinned page-title fixture records the source helper and page composition
excerpts, hashes, synthetic inputs and expected output. The isolated PHP runner
executes only the captured helper after checking its hash. It requires PHP with
mbstring. Expected vectors were transcribed from the source; execution against
the PHP oracle is a separate qualification step. Local Rust tests do not stand
in for that step.

`yotsuba_config.php` obtains default titles from the source runtime `boardlist`
database. The checkout's static directory names support the imported names but
do not prove every historical runtime heading. Explicit board `TITLE` overrides
provide stronger evidence for those boards. No missing homepage or template is
reconstructed here.

Migration 0120 corrects only the known imported `/s4s/` title when its exact old
value remains. Operator edits are preserved. Migration 0045 and the earlier
encoding correction stay unchanged. Navigation grouping, directory configuration
and worksafe flags are outside this change.

## Checks

```sh
npm run test:page-identity-core
cargo test -p board-domain page_title
cargo test -p board-public --features database-tests --test page_identity
python3 scripts/extract-page-title-reference.py --fixture-check fixtures/page-title-reference.json --php php
```

The HTTP tests use a migrated disposable database and check rendered titles,
visible headings, archived threads, Unicode boundaries and escaped hostile text.
The populated-upgrade check compares board state, content and permissions before
and after the title-only correction, then verifies an operator-edited title.
Browser screenshots are not refreshed by this change.
