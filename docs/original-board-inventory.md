# Original board definitions

Migrations 0044 through 0046 install all 80 entries in `4chan-old/boardlist.txt`
and the configuration-only `/asp/` and `/qb/` boards. The pinned checkout is
revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
[The fixture](../fixtures/board-reference.json) records the source-file hashes,
names, descriptions, order, category and effective supported settings.

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

Installed policy includes comment character/line limits, spoilers, code/SJIS
tags, required subjects, OP markup, forced anonymity, IDs, country flags,
text-only behavior, bump/image limits, index capacity and pagination, archive
retention, JSON tail size and bump age. Catalog and JSON switches control their
routes and navigation. `/f/` has no ordinary catalog. Its specialized upload
workflow remains in #206. Source flags for unsupported policies are recorded
in the fixture and tracked through #192; recording a flag does not implement it.

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
  --board-encoding-migration migrations/0064_board_reference_encoding.sql --check
```

`board_inventory.rs` checks every installed field and public route with actual
database credentials. It creates healthy private rows, proves that public reads
and writes are denied, and verifies directory links. The migration upgrade test
checks historical data, new stamping and restricted insertion in a disposable
database. These checks cover inventory and supported policy, not all specialized
board behavior or original homepage pixels.
