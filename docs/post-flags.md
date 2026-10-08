# Country and board flags

Flag-enabled boards save a public display label when a post is created. The
posting transaction checks the current board policy under its existing board
lock. Geographic posts use the verified network peer and an offline country
database. A selected board flag must appear in the operator's allowed list.
Both text and approved attachment posts use the same transaction and trigger.

The [pinned API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Threads.md)
defines the conditional post fields `country`, `country_name`, `board_flag` and
`flag_name`. Its board contract defines `country_flags` and `board_flags`.
The public extension renders `bfl bfl-{code}` or `flag flag-{code}` spans with
escaped names as titles. [The reference record](public-country-flags-reference.json)
pins the stylesheets, sprites, dictionary and observed select order.
[The source flag inventory](source-board-flags.md) records all four supplied
tables, their separate menu/post labels and the current rendering policy.
No external posting experiment or production user-content fixture was used.

## Configuration

Migration 0041 defaults `country_flags` to false and `board_flags` to an empty
array. Existing posts receive nullable fields; their other values are preserved.
Configure boards through the migration/operator identity, outside public routes.
For example, on an owned board:

```sql
UPDATE content.boards
SET country_flags=true, board_flags=ARRAY['AC','UN']
WHERE slug='demo';
```

Set `COUNTRY_DATABASE` to an absolute path to a GeoIP2-Country or
GeoLite2-Country MMDB file readable by the public service. Obtain and maintain
that database under its supplier's terms. Production data and download
credentials are not included in the repository. The synthetic test database
must not be used for production geolocation.

The public process loads a regular file of at most 64 MiB, verifies its structure
and keeps an immutable memory copy. Invalid configuration rejects startup.
There is no automatic download, network lookup, memory mapping or unchecked
UTF-8 decoding. Replace the operator-owned file and restart the public service
to apply an update. Existing post labels stay unchanged. Keep the file outside
the public service's write authority and outside the worker's job storage.

Enabling geographic flags without a loaded database rejects geographic posting.
The application also rejects missing verified peers and failed lookups; it does
not fall back to client headers. A valid lookup with no country returns `XX` and
`Unknown`. IPv4-mapped IPv6 peers use canonical IPv4. The country record supplies
the label; the registered-country record is not a fallback.

Only the 25 pinned board-flag codes can be configured. Boards may enable a
subset. The ordinary form and Quick Reply offer those choices in the pinned
select order, starting with `0` for geographic location. Invalid, disabled or
lowercase choices are rejected. The HTML form, URL-encoded and multipart
posting handlers, and inline Quick Reply preserve the selected code.

## Public representation and privacy

Full thread, index and catalog JSON expose a geographic pair or a board-flag
pair on each qualified post. Tail JSON retains its minimal OP metadata and
includes labels on returned replies. Disabled boards omit the board switches.
Historical posts without a captured label omit the post fields. Public HTML,
updater snapshots and quote previews use the saved labels and fixed local
sprites. The native parser accepts only finite sprite classes, empty flag spans
and bounded titles; it still rejects event handlers, inline styles and arbitrary
attributes.

Selecting a board flag stores only that choice and omits the geographic pair.
The released client establishes display precedence, but it does not establish
the original server's capture or retention rules. This omission is an explicit
project privacy policy, E-015. Likewise, the offline data source and lookup
algorithm replace undocumented private infrastructure; country accuracy is not
claimed to match the reference deployment. Flags describe network geography or
a public choice, not a person's identity or physical location.

No new raw peer address is saved for this feature. `CF-IPCountry`, forwarding
headers and posted country/name fields cannot select geographic labels. Public
SQL credentials cannot change the board policy or update saved flag columns.
The attachment owner gains SELECT on the public board switches and flag type;
staff authentication, private poster contexts and deployment grants are unchanged.

## Qualification

The producer's synthetic fixture checks GB versus its different registered
country, JP over IPv6, IPv4-mapped equivalence and the unknown branch. Actual
public-role HTTP tests cover both JSON listeners, full/tail/index/catalog
responses, HTML, updater and quote-preview rendering, missing sources/peers,
header spoofing, invalid choices, unknown fields, column-update denial and
transaction-local context reset after disabling the policy. Both attachment
modes also check persisted flags alongside trips, IDs and private counts.

The owned browser test uses a real loopback socket: a script-free geographic
post ignores spoofed headers and renders `Unknown`; Quick Reply selects an
allowed flag, persists it and receives it through the actual updater. The test
checks title, dimensions, unknown sprite positioning and JavaScript errors.
Local quote previews retain both finite flag classes and saved titles in the
source module and released bundle. All 45 preview/settlement cases passed,
including malformed local flags rejected before DOM construction.
Parser cases reject unknown classes, mixed classes, children, oversized titles,
controls and active attributes. These checks establish the implemented slice;
complete original-page and mobile visual parity remain unfinished.

Run `cargo test -p board-public --all-features --test post_flags --locked` and
`cargo test -p board-store --all-features --test post_media --locked` against an
owned migrated database with the test identities configured. The browser test
also needs the pinned Node, installed package lock and Chromium. Fresh migration
and role checks run through `sudo bash scripts/test-role-bootstrap.sh`.
