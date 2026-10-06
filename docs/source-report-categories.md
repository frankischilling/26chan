# Report category configuration

This slice adds a pure category selector and private, immutable catalog imports.
It does not activate category reporting. There is no activation pointer or
command, no seeded production catalog, and no change to the current free-text
report form or staff queue. Importing a catalog does not make it live.

## Source contract

`4chan-old/modes/report.php:307–395` reads nine fields from `report_categories`:
`id`, `board`, `op_only`, `reply_only`, `image_only`, `exclude_boards`, `title`,
`weight`, and `filtered`. The DDL in `4chan-old/README2.txt` is explicitly a
reconstruction from PHP; its types, lengths and defaults are not authoritative.

The selector keeps these source rules:

- ID 31 is the illegal category and bypasses every category scope filter.
  Without that row, there is no illegal selection or invented label.
- An empty `board` string is global. NULL does not match an ordinary category.
  `_ws_`, `_nws_`, and exact board names select their corresponding scopes.
- OP means `resto == 0`; image presence means nonzero `fsize` and false
  `filedeleted`. OP-only, reply-only and image-only flags restrict rule entries.
- Board exclusions match comma-delimited names exactly, without trimming or
  case folding. PHP's falsey string `"0"` is preserved for exclusions and scope
  grouping; scope `"0"` still needs an exact board match.
- Matching truthy scopes appear before global/falsey scopes. Within each group,
  input order is preserved. The source orders by `board ASC` without a tie-breaker;
  its collation and equal-board ordering are unknown.

Catalog arrays must therefore carry the intended source row order. The importer
does not sort by ID, title, or the destination database's collation.

The source form (`forms/report.php:167–217`) submits `cat` and `cat_id` to its
current `imgboard.php?mode=report&no=...` URL. The handler reads the target from
the query string. A truthy `cat` wins over `cat_id`; absent, empty and exactly
`"0"` are falsey. A malformed winning value must not fall back to the other
field. The pure helper selects only the raw string. PHP numeric coercion and
strict decimal parsing remain separate compatibility choices.

## Import boundary

Validate a local configuration file without connecting to a database:

```sh
cargo run -p board-store --bin board-report-catalog -- validate catalog.json
```

Import using the operator's `MIGRATION_DATABASE_URL`:

```sh
cargo run -p board-store --bin board-report-catalog -- import catalog.json
```

The JSON envelope is `{"version":1,"categories":[...]}`. Use `--help` for the
field types. Validation rejects missing, unknown and duplicate fields, as well
as NUL characters. The importer requires both the database session user and
current user to be `board_migrator`.

Catalog import and readback are migrator-only operations. The SQL entry point is
`content.import_report_catalog(jsonb) -> bigint`; it returns a private immutable
version. The importer reads it through `content.read_report_catalog(bigint) ->
jsonb`, compares the typed fields and row order, and commits only after a match.
Weights use finite numeric equality; JSONB may normalize negative zero to zero,
so this is not a promise to preserve floating-point sign bits.
Public, staff and authentication runtime roles cannot import catalogs.
Imports use Read Committed and a private import gate; they do not lock the
current report-admission gate or change live reporting.
Every row supplies all nine fields explicitly. NULL and empty strings remain
distinct; category IDs must be positive and unique, and weights must be finite.
No finite weight range or filtering-threshold default is inferred from the source.

These are rewrite resource limits, not recovered production settings:

- Raw input: 8 MiB; normalized SQL JSONB: 8 MiB
- 4,096 rows per catalog; 64 retained versions
- Board text: 256 UTF-8 bytes; title: 4,096 bytes
- Exclusion text: 65,536 UTF-8 bytes

Import validation fails rather than truncating fields, discarding rows, or
evicting old versions. Raw-file and normalized-JSONB limits are separate checks;
an input within the file limit can still exceed the database representation limit.
Offline validation does not establish available database version capacity.

## Verification and remaining work

The domain tests use synthetic categories. They cover NULL/empty scopes,
worksafe groups, order preservation, exclusions, OP/reply/image restrictions,
illegal-ID bypass, missing selections, duplicate IDs, nonfinite weights and raw
field precedence. They preserve configured weight/filter metadata without
calculating a reporter's effective weight. Test results belong in the checkpoint;
this document does not claim they passed.

Before activation, categorical submission must recheck the selected row against
the current board, post and catalog inside the existing report-admission
transaction. Catalog changes must serialize with that check, and the report,
duplicate/quota membership and anonymous-session activity must commit together.
Image checks need the stored attachment size and deletion flag, not the public
media view's asset-availability fallback. Existing free-text reports must remain
readable, while every mutation entry point must enforce the selected reporting
mode. None of that integration is provided by a catalog import alone.

Sanitized original category rows are still needed for labels, scopes, base
weights and filtering thresholds. Queue weighting also depends on missing
`report_settings` board coefficients and unresolved source query details.
Personal records, credentials, accounts and report histories are not needed to
supply this configuration. CAPTCHA, Pass, trust-adjusted weights, legacy report
POST integration and weighted/grouped staff queues remain outside this slice.
