# Source navigation

The static navigation reference is revision `545b7812d1849f7958d914950c91fdbbe38f6b22` of the operator-supplied source checkout. `fixtures/navigation-reference.json` preserves the raw header and directory source text, file SHA-256 hashes, normalized behavioral excerpts, and extracted labels. `scripts/extract-navigation-reference.py` checks those hashes before generating `apps/public/src/views/navigation_data.rs`. It reads source text without executing PHP, JavaScript, application initialization, or database queries.

## Header and directory contracts

`header.txt` and `header-ws.txt` contain the same 77 ordered board anchors in five bracket groups of 27, 2, 4, 4, and 40. The two files have different DOM structure. `header.txt` has no `nwsb` wrappers. `header-ws.txt` has 22 such spans covering 23 anchors: `b d e f gif h hr r s t u wg i ic r9k s4s hm y aco bant hc pol soc`. The `i` and `ic` anchors share one span; some spans also contain separators or brackets. That file uses cross-domain links for those anchors. Group order, slug, and decoded title enter `HEADER_GROUPS`. The fixture separately records each anchor's immediate-parent `nwsb` classification for both headers; wrappers and domains do not become local board policy.

Core derives each mobile option's `nwsb` class from its anchor's immediate parent. Matching those classes therefore depends on which header variant the page uses. Equal memberships and labels do not imply equal DOM. Hash-pinned assignments in `config/global_config.ini:315` and `config/boards/test.config.ini:167` both point `NAV_TXT` at `header.txt`. The checked-in configured default is therefore established. The generated `HEADER_NWS_SLUGS` follows that default and is empty. Local `Board.worksafe` remains separate policy data; it does not add source wrapper classes. Historical runtime overrides and deployment-time selection of `header-ws.txt` remain unknown.

Header labels include `p` = `Photo`, `diy` = `Do It Yourself`, `lgbt` = `LGBT`, and `s4s` = `Shit 4chan Says`. HTML entities are decoded once: `qa` is `Question & Answer`, and `vp` is `Pokémon`. The renderer must escape these strings for its output context.

`www.4chan/data/boards.php` contains 78 ordered records across the `img` and `upload` arrays. `DIRECTORY_LABELS` preserves that order, including the final `f` record. The directory includes `asp` and `trash`, which the header omits, and omits the header's `qa` entry. Its labels for `p` and `diy` are `Photography` and `Do-It-Yourself`. Both lists exclude `test`, `qb`, and `j`.

The 77 header entries, 78 static directory records and 82 installed board policies are separate sets. The existing authorized store and directory API bound remains 100 entries. Header rendering intersects the source groups with the authorized local snapshot and omits empty groups. A current board absent from the header uses a separately marked mobile fallback without becoming a source-header member.

The header groups are static presentation data. A directory may use the reference label for an authorized local board that appears in `DIRECTORY_LABELS`. Authorized local boards absent from that static reference remain eligible for directory fallback using their local slug and database title. A reference entry alone does not authorize discovery or publication of a board. No imported policy flag, title database row, or migration is changed by the extractor.

## Modes and footer

The fixture includes these hash-pinned source excerpts, with CRLF normalized to LF before excerpt hashing:

- `js/core.js:912–919`: mobile selection preserves catalog mode except for `f`; all other page modes, including archive, select the board index.
- `js/core.js:921–955`: mobile options are built from desktop board anchors, sorted lexically by slug. Each label is `/{slug}/ - {title}`.
- `js/core.js:956–978`: the footer clones the desktop navigation, changes its ID to `boardNavDesktopFoot`, renames the utility and settings IDs, and inserts it before `absbot`.
- `catalog.php:192–193`: header links target `/catalog`, except `f`, which retains its board-index link.
- `imgboard.php:3191–3196`: archive header links target `/archive`, except `f` and `b`, which retain their board-index links.

Local archive pages render source groups and the mobile selector on the server and load only the fixed page-chrome module. They do not mount the native persistent-navigation controller. The source archive-link rules describe destinations and do not imply broader script authority.

These excerpts establish the source operations. The extractor tests do not execute them in a browser. Runtime and browser tests must separately verify the local renderer, mobile behavior, footer, and mode transitions.

## Regeneration and checks

Run from the repository root:

```sh
python3 scripts/extract-navigation-reference.py --source /path/to/26chan-reference --write
python3 scripts/extract-navigation-reference.py --source /path/to/26chan-reference
python3 scripts/extract-navigation-reference.py
python3 scripts/test-navigation-reference.py
```

The write command requires a source checkout whose eight input files match the pinned hashes. It writes only the navigation fixture and Rust table. The default check verifies captured evidence and checks both generated files byte for byte without requiring that checkout. Rust strings escape quotes, backslashes, and control characters; non-ASCII labels retain their Unicode text. Generated constants skip rustfmt so formatting remains deterministic.

The 17 regression tests cover configured header selection and parent-wrapper classification, mobile destination evidence, source and excerpt tampering, fixture metadata and content drift, group sizes, worksafe equivalence, duplicate slugs, internal-board exclusions, decoded labels, header/directory differences, source record order, and deterministic Rust escaping.

## Evidence limits

The source checkout does not supply the production runtime database titles or the generated homepage board-list HTML. This fixture therefore establishes static header labels and directory source records, not production database content, homepage layout, or full historical page-visual parity. A local directory's fallback title is local data. Browser checks against synthetic local pages establish local behavior only; production screenshot parity requires separate reference captures.

## Local qualification

The final local candidate passed all 17 extractor tests and regeneration checks against the supplied checkout. Six Rust projection/rendering tests cover complete and partial groups, empty headers, labels, destinations, privacy, escaping and the directory bound. Actual PostgreSQL-backed HTTP and concurrent-snapshot tests passed, as did the board inventory, subtitle and page-title regressions. The public library suite passed 135 tests; one Unix-domain-socket test could not run in this environment and was explicitly filtered. All-target/all-feature public checks, Clippy with warnings denied, formatting and generated-asset checks passed.

Pure JavaScript admission and transport checks passed. Browser and theme cases were discovered, but local Chromium is unavailable. Hosted execution, screenshot review and exact-head CI remain required before merge. No imported board definition, stored title, visibility rule, API schema or migration changed in this slice.
