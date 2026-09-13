# Catalog controls

Issue #64 adds working sort, thumbnail-size, teaser and search controls to the
catalog cards. The [public reference](public-catalog-reference.json) records
the observed option labels/values and pinned client semantics. Its reproduction
script now checks all four display modes in every theme at desktop/mobile
widths. It never executes the original client or uses production post fixtures.

## Read interface

`GET /{board}/catalog` accepts these optional parameters:

| Parameter | Values and default |
| --- | --- |
| `order` | `alt` (default, bump order), `absdate` (last visible reply), `date` (creation), `r` (visible reply count) |
| `size` | `small` (default) or `large` |
| `teaser` | `on` (default) or `off` |
| `q` | Literal case-insensitive filter, at most 128 Unicode characters; empty by default |

The raw query is limited to 2,048 bytes. Unknown or duplicate parameters,
unknown option values, oversized filters and control characters produce a
generic 400 response before database access. The query is never interpreted
as regex, SQL, HTML, CSS or a URL. Search values are escaped when redisplayed.
Other existing read routes and JSON query handling are unchanged.

All sorting/filtering uses the same bounded board snapshot as the rendered
counts and OPs. The count query also reads the maximum visible reply ID in
that transaction. A sage reply affects last-reply order without bumping its
thread; deleted replies affect neither visible counts nor last-reply order.
Sticky threads stay first in each ordering. Count/last-reply ties use ascending
thread ID, matching numeric enumeration before the public client's stable
sort. Bump-order ties retain the existing descending-ID rule.

Search covers each OP's subject, comment and undeleted filename. Replies are
not searchable through this control. A filtered-empty catalog says that no
threads match and provides a working reset link, rather than claiming the
board is empty. Reset clears every option. Selected values survive reloads
through the URL and remain usable without JavaScript.

Small thumbnails use attributes bounded to 150px; large uses 250px, including
bounded legacy full-image previews. Small cards are 165px without teasers or
180px with them, both 155px below 481px. Large cards are 270px. Teaser-on cards
have the referenced 320px/410px height limits; keyboard focus expands them.
Teaser-off cards omit teaser markup. All modes retain escaped filenames and
never request spoiler/deleted-file image bytes.

## Differences and remaining work

The public client applies options immediately and stores preferences/search
in browser storage. This implementation submits a GET form and keeps options
only in its URL. It retains the current `script-src 'none'` boundary and does
not make user-controlled parameters executable. Search uses each stored field
and Unicode lowercasing, not the original generated-teaser markup or JavaScript
regex casing rules. Original teaser preprocessing, preference persistence,
instant filtering, watchers, menus and hover previews remain incomplete.
[Catalog fallback graphics](catalog-state-assets.md) are tracked by #66;
other page-level visual gaps remain under #6.
Toolbar wrapping is local: each label stays with its control on narrow screens.
These controls do not establish whole-client or whole-page parity.

## Verification

The actual-role integration test covers all four sorts, sticky ordering,
literal punctuation/hostile search text, invalid queries, deletion and selected
values. Media tests verify filename matching and its removal after file
deletion. The concurrent-commit test now includes each alternate ordering and
a filtered catalog. A no-JavaScript browser posts three owned threads and sage
replies, operates all controls, reloads selections, resets and deletes its
fixtures through the real handlers. Six theme cases check all four display
modes at both widths, exact thumbnail attributes, keyboard focus and label
alignment. Six additional screenshots cover the nondefault display modes.

Final local checks passed on owned Windows/PostgreSQL 16.15:

```text
cargo test -p board-public --all-features --locked --jobs 1 --quiet
cargo clippy -p board-public -p board-store --all-targets --all-features --locked --jobs 1 -- -D warnings
npm run test:behavior
node scripts/verify-public-catalog-reference.mjs .local/reference/catalog-20260913
cargo fmt --all -- --check
python scripts/check-media-parser-dependencies.py
```

All 56 public tests and ten real-server browser scenarios passed. Sequential
ordinary `test:visual`, `test:archive-visual`, `test:media-visual`, `test:states`
and `test:themes` runs passed 53 scenarios and 45 screenshot comparisons with
`VISUAL_FIXTURE_SERVER=1`. Five existing catalog captures and six new mode
captures were individually inspected; all eleven accepted PNGs match the
final reviewed captures byte-for-byte. The other 34 baselines are unchanged.
JavaScript syntax checks passed. Browser pins, zero retries and zero-pixel
tolerance are unchanged. PR #65 merged as
`0cf5d94bb2b3ec1b0cf55043857b91be0d7a8d3e` after all six checks passed on
`b448b56506c2d0018117b39d9a2c8955854f4b8b`: [PR build and native qualification](https://github.com/frankischilling/26chan/actions/runs/34744428010),
[push build and native qualification](https://github.com/frankischilling/26chan/actions/runs/34744425898),
[PR monitoring](https://github.com/frankischilling/26chan/actions/runs/34744428021)
and [push monitoring](https://github.com/frankischilling/26chan/actions/runs/34744426010).
The build workflows include Windows visual checks. The merged-main build and
monitoring workflows also passed. These disposable-runner results do not
qualify the production deployment.

Initial failures were confined to test scaffolding and expected screenshots:
SQLx rejected dynamically composed fixture-cleanup SQL, which now uses fixed
statements; an incomplete fixture INSERT lacked mandatory board limits, which
are now explicit. Screenshot review caught a mobile label wrapping away from
its field; grouped controls and a regression assertion fix that layout.

The wider Windows `cargo test -p board-store --all-features --locked --jobs 1`
run passed 12 tests, then failed at the monitoring
test's explicit `/tmp/board-postgres.*` cluster marker. That guard was not
altered or spoofed, and subsequent store tests did not run in that command.
Hosted Linux CI remains required for the full store and native boundary suite.
This slice changes no dependencies, migrations, credentials, service authority,
production enablement or workflow permissions.
