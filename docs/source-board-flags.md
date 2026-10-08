# Source board flags

The supplied source defines four flag tables. Migration 0083 imports the two
enabled sets and leaves the other two disabled, matching all 82 INI policies.
Only migration authority can change a board's type and allowed choices.

| Type | Choices | Source default | Retrieved artwork |
| --- | ---: | --- | --- |
| `pol` | 25 | Enabled on `/pol/` | Version 2 sprite, already served locally |
| `mlp` | 83 | Enabled on `/mlp/` | Version 3 sprite |
| `lgbt` | 55 | Disabled | Version 1 sprite |
| `test` | 2 | Disabled | Version 1 stylesheet unavailable, HTTP 404 |

[The PHP reference](../apps/public/tests/fixtures/board-flags.json) records the
actual display dictionaries, selector labels and selector order, including the
disabled tables. Political menu and post labels differ: `BL` is "Black
Nationalist" in the menu and "Black Lives Matter" on the post; `CM`, `GY` and
`TM` also have distinct labels. Codes can mean different things across types:
`AN` means "Anarchist" in `pol` and "Anon" in `mlp`.

Ordinary and Quick Reply forms use the source menu labels and order. The first
option is "Geographic Location" when geographic flags are enabled, otherwise
"None." `/boards.json` preserves that order inside its `board_flags` object.
Post HTML and JSON use the current source display dictionary when the board
enables flags. Disabling flags suppresses their public projection. Staff posting
updates its allowed menu when the selected board changes.

Ordinary forms and Quick Reply remember choices under the source key
`4chan_flag_<board>`. Each form restores only an option in its current allowed
menu. Quick Reply reads the preference again when reopened. Selecting the first
option removes the saved choice; other boards retain their own preferences.
The ordinary form keeps this behavior when the extension is disabled. Unknown
or malformed saved values leave the current default intact. Storage failures
leave the form usable, and saved strings never become CSS selectors.

The posting transaction locks the current board policy and accepts only its
allowed, case-sensitive codes. The database trigger captures the type and
display label independently of browser input. Migration 0083 preserves existing
post fields and gives existing flag records the `pol` type used by the previous
schema. Saved labels remain intact; public rendering uses the current dictionary.
The captured type is internal and is not an added public JSON field. Badge posts
continue to suppress both geographic and selected flags.

Nonpolitical spans keep the source `bfl bfl-{code}` classes and add a fixed type
class. The stylesheet scopes each sprite and coordinate to that class, so a
cross-board copy cannot confuse shared codes or inherit political dimensions.
Native previews, inline quotes, updates, expansion and search validate the
complete finite class combinations before creating DOM nodes. Test flags have
no substitute sprite and cannot display the first political image by accident.

[The asset record](source-board-flag-assets.json) contains collection times,
source URLs, hashes, byte lengths and all 163 available sprite coordinates and
dimensions. PNG bytes are unchanged. Source CSS is scoped and points to fixed
local routes; the public runtime does not contact the CDN. These retrieved files
do not establish historical pixel identity. The country provider and its security
replacement remain documented in [country and board flags](post-flags.md).
Selected flags retain that document's omission of geographic capture; disabling
selected flags cannot recover geography that was never saved.

Apply migration 0083 before starting these binaries. It checks known types,
canonical arrays of at most 83 codes, null entries and cross-type choices.
The new label function has execution grants only for the four posting identities;
it grants no write authority. Public readiness requires the type column. Before
rolling back to a binary that supports only political flags, disable nonpolitical
choices through migration authority. Do not reverse the schema to discard history.

## Verification

The populated upgrade compares historical data, privacy policies and existing
function/table/column authority. Only the documented policy imports, new columns
and flag-trigger body may differ. It checks the distinct labels, array failures
and seven actual runtime write denials. Public HTTP checks post all 165 choices,
verify both JSON listeners and HTML, compare all 82 policies, check source menu
and JSON order, and verify suppression after disabling flags.
Staff HTTP checks also verify type-specific menus, rejected cross-type choices,
captured labels and public projection. A browser check changes the selected board,
compares every menu with the source and clears an unavailable prior choice.
Desktop and mobile browser checks cover remembered choices, numeric and
three-character codes, Quick Reply reopening, reloads, removal, disabled
extension settings, unavailable storage and invalid saved values. The source
preference verifier executes both supplied restorers and the actual change
callback against all 165 choices with synthetic storage.
The full role bootstrap also compares every pre-flag historical post field in
both directions and requires the new captured type to be `pol` on those rows.

Browser contracts render every available flag through production templates on
desktop and mobile at density two. They compare source coordinates, dimensions,
labels and menu order, then send the actual post HTML through the released parser
worker. Fixed bytes and the complete image CSP set have separate route checks.
The worker keeps its 256 KiB limit: only parse5's directly called internal
`_state*` tokenizer methods are shortened. Build checks reject matching application
identifiers or quoted property lookups. Parser and client regressions remain required.

```sh
python3 scripts/extract-board-flags-reference.py 4chan-old \
  apps/public/tests/fixtures/board-flags.json --check \
  --rust crates/domain/src/board_flags.rs \
  --client apps/public/client/native-board-flag-codes.js \
  --migration migrations/0083_source_board_flags.sql
python3 scripts/prepare-board-flag-assets.py /path/to/collected-assets --check
node scripts/verify-source-flag-preferences.mjs 4chan-old/js/core.js 4chan-old/js/extension.js
sudo bash scripts/test-source-board-flags-migration.sh
cargo test --locked -p board-public --all-features --test post_flags --test ui_assets
npm run test:source-flags
```

Issue #217 remains open for the unavailable test artwork, the two custom-spoiler
images, and complete original-page/historical visual qualification. The available
flag inventory and rendering do not establish the full rewrite's completion.
