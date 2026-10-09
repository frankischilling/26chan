# Original board definitions

Migrations 0044 through 0046 install all 80 entries in `4chan-old/boardlist.txt`
and the configuration-only `/asp/` and `/qb/` boards. The pinned checkout is
revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
[The fixture](../fixtures/board-reference.json) records the source-file hashes,
names, descriptions, order, category and effective supported settings.

The source runtime takes its default title from `boardlist.name` in the database
(`yotsuba_config.php:86-102`) before applying category and board configuration.
That database snapshot is not supplied. The importer uses the static directory
names in `www.4chan/data/boards.php`, with configuration `TITLE` overrides and
description fallbacks for unlisted names. These imported defaults do not prove
the source runtime database names were identical.

The extractor applies global, category and board overrides in that order.
The migration upserts definitions without replacing posts, threads, credentials
or saved formatter profiles. Existing synthetic boards remain available.
Fresh setup runs these migrations before the optional demo seed.

Migration 0063 imports the source wordfilter switches and fixed board profiles;
79 definitions enable filtering and three disable it. Migration 0064 corrects
the UTF-8 Pokémon title that the old Windows-default extractor decoded
incorrectly. It preserves the applied import checksum and operator-edited
titles. The extractor now reads board names explicitly as UTF-8 and verifies
the historical import together with that additive correction.

Migration 0120 corrects the stored `/s4s/` short name to `Sh*t 4chan Says`.
Its source override is `TITLE = [s4s] - Sh*t 4chan Says`; the importer previously
removed only slash-form prefixes. The update requires both slug `s4s` and the
exact old title, so operator edits remain intact. The fixture keeps the full
override in `source_policy.TITLE` and the short name in `title`. Historical
title extraction remains separate from the current import so regeneration
preserves migrations 0045 and 0064 byte for byte.

Migration 0069 imports all 82 authorized comment budgets: 10,000 characters on
81 boards and 50,000 on `/j/`. Its separate hash-pinned
[fixture and qualification](verification-authorized-post-limits.md) cover
moderators, managers and administrators; janitors retain ordinary limits.
The private proof owner can read only the board slug and two comment budgets,
including `/j/`, and cannot change policy. This adds no public directory entry
for the private board.

Installed policy includes comment character/line limits, spoilers, code/SJIS
tags, required subjects, OP markup, forced anonymity, IDs, country flags,
text-only behavior, bump/image limits, index capacity and pagination, archive
retention, JSON tail size and bump age. Catalog and JSON switches control their
routes and navigation. `/f/` has no ordinary catalog. Its specialized upload
workflow remains in #206. Source flags for unsupported policies are recorded
in the fixture and tracked through #192; recording a flag does not implement it.

Migration 0114 imports drawing and replay switches plus 400 x 400 default
dimensions for all 82 boards. `/i/`, `/qst/` and `/vip/` enable drawing in the
source; only `/i/` enables replay. The [native PNG drawing slice](native-drawing.md)
supports `/qst/` and `/vip/` when the existing media gates pass. `/i/` remains
runtime-disabled pending replay support; recording its policy does not complete
its workflow. The unused source `OEKAKI_MIN/MAX` constants are not imported as
admission limits.

Migration 0092 imports the executable `MAX_USER_THREADS` and
`MAX_USER_THREADS_PERIOD` settings as `user_thread_limit` and
`user_thread_period_hours`. The source defaults are five OPs per board over
24 hours. `/a/`, `/bant/`, `/v/`, `/vm/`, `/vmg/`, `/vrpg/` and `/vst/` use
three over 24 hours; `/i/` uses three over 168 hours, `/pol/` three over six,
`/qa/` three over 48, `/qst/` five over 72, `/news/` five over 120, and
`/test/` 50 over 24. The inventory fixture and database assertions cover all
82 definitions, including private and configuration-only boards. A configured
zero limit denies OP creation; it does not disable this quota.

`/j/` is installed as a private board. Row policies protect boards, threads,
posts, reports, deletion credentials and OP peer metadata. Public, attachment
and counting credentials cannot bypass those policies through ordinary queries.
Shared media projections enforce visibility even when their view owner can
bypass row security. Authorized staff credentials can read private content;
board-scoped staff permissions and the private-board interface remain in #221.
The public directory omits `/j/` and keeps the original order of visible boards.

The extractor retains the application's 1,000-thread response ceiling for
source configurations with an unbounded index. It also keeps the existing
1,000-reply storage ceiling. Those resource limits require a separate bounded
staff history interface and explicit policy coverage under #192 and #221.
They do not change the recorded original policy values in the fixture.

To compare the fixture and migration with the supplied checkout:

```bash
python3 scripts/extract-board-reference.py 4chan-old fixtures/board-reference.json \
  --migration migrations/0045_original_boards.sql --rss-migration migrations/0047_rss_feeds.sql \
  --wordfilter-migration migrations/0063_post_wordfilters.sql \
  --board-encoding-migration migrations/0064_board_reference_encoding.sql \
  --board-title-migration migrations/0120_board_short_titles.sql --check
```

`python3 scripts/test-board-title-import.py` checks prefix handling, independent
historical encoding and title extraction, CLI regeneration and the guarded
title update. Its in-memory SQL check covers unchanged operator titles, other
boards and descriptions, including a repeated application of migration 0120.
`sudo bash scripts/test-board-titles-migration.sh` applies the correction to a
populated disposable PostgreSQL database and checks all board fields, retained
posts and threads, ownership, grants and row policies. It reapplies the migration
to both the corrected title and an operator-edited title.

`board_inventory.rs` checks every installed field and public route with actual
database credentials. It creates healthy private rows, proves that public reads
and writes are denied, and verifies directory links. The migration upgrade test
checks historical data, new stamping and restricted insertion in a disposable
database. These checks cover inventory and supported policy, not all specialized
board behavior or original homepage pixels.
