# Optional source events

Issue #224 covers event presentation and optional local event behavior. The
reference snapshot does not enable every historical event. An event being
present in a PHP file is not evidence that its endpoint or posting hook runs.

## Reachable behavior

- `PARTY` defaults to false. Its image choice defaults to `xmashat.gif`.
  The ordinary board renderer inserts a hat before each opening post when
  enabled, and the catalog exposes the same image choice to its client.
- `CSS_EVENT_NAME` is absent from the active configuration. When configured,
  board and catalog pages add an event stylesheet. The `tomorrow` event has
  snow and garland behavior; `spooky` adds desktop skeletons and a dark CAPTCHA
  hint. The ordinary Tomorrow theme is not itself an instruction to enable
  the Christmas event.
- The HTML renderer recognizes stored April 2024 identity values from 10000
  through 10017. The main JSON serializer removes those values instead of
  exposing them as Pass purchase years. The separate test serializer differs;
  it is not the production JSON contract.

## Disabled and disconnected branches

- `emotes_xa22.php` returns HTTP 404 and `File not found.` before its controller
  and database code. Its roll, purchase and session actions are unreachable.
- `xa24tb.php` returns `{"error":"The market is closed"}` before dispatching
  account creation, buying or selling. The board's stock-email admission and
  investor-name assignment are commented out. These are fictional event
  balances, not financial accounts or real securities.
- The main posting hook for April 2019 scores is commented out. No supplied
  PHP entry point includes `lib/like_score.php` or `lib/perk_options.php`.
  The score getter in the former returns zero before its database lookup.
  Its active perk table assigns a zero-point threshold to every listed perk;
  older thresholds and several options are comments, not active eligibility.
- Halloween 2017 score functions and their posting/moderation hooks are inside
  block comments. The April 2018 team-score call is a line comment. The
  Christmas fortune replacement is also commented out; ordinary fortunes
  remain the active configuration-dependent behavior.

## Implementation boundary

The rewrite must preserve the closed responses and ordinary-board defaults.
Opt-in reconstructions of dormant event controllers need explicit operator
activation, bounded persistent state, private actor identities, atomic votes
or trades, and tests for expiry, duplicate requests and ownership. They must
not reuse historical secrets, user records or external payment services.

Event image and stylesheet files referenced by the source must be inventoried
separately from their controllers. Missing artwork is not proof of visual
parity and must not be replaced by unrelated images without being identified.
With workflows disabled, the two closed endpoints do not consult a database,
read event cookies or create accounts. Eighteen independently executed source
action cases are replayed through GET, HEAD and same-origin POST. Cross-origin
writes retain the common refusal. The emote error uses plain text rather than
PHP's implicit HTML content type; response text and status are unchanged.

The isolated helper corpus covers 94 stock-identity, investor-name, Halloween
badge and perk-option cases. It executes pinned slices with PHP 8.3; the
historical production PHP version is unknown. Stock valuation is supplied as
a scalar, and the score-read boundary retains the source's unconditional zero.
These tests do not establish an active scoring, market or emote workflow.
Optional market/emote activation and private persistence are implemented below.
Scoring, seasonal presentation and final browser qualification remain open.

## Dormant market model

The pure Rust model preserves the 18 symbols, 1,000-point starting balance,
1,000-unit order ceiling, ten-second cooldown, stale-price refusal and public
balance projection. A separate corpus executes 108 cases through the original
buy/sell functions against an owned in-memory ledger. It covers scalar coercion,
validation order, zero holdings, insufficient funds and exact cooldown edges.
No historical account or quote data is used.

The model returns a complete next account rather than changing half a trade.
Overflow and unsupported symbols are rejected. The optional persistent controller
authenticates the actor, locks the run, quote and account, and rechecks the price
before applying either side of a trade. The endpoint remains closed by default.
None of this code connects to financial services or handles real money.

## Dormant emote model

The actual source roster has 166 entries: 58 Unicode choices, 82 ordinary
images and 26 rare images. Its nearby comments give older pool counts; the
literal arrays, not those comments, define lookup and selection. Image choices
use their eight-digit asset identifiers, while Unicode choices use their names.
The generated Rust roster is checked against 172 source lookups and 24 balance
cases. No downloaded image is implied by an entry in that roster.

The model retains 20 points per completed five-minute interval, stored balances
that exclude those elapsed-time credits, 3/10-roll charges, duplicate recycling
and the 50/150/300-point purchase prices. A 393-case corpus executes the original
buy and seeded-roll functions against an owned ledger. Rust replays their
recorded results, including duplicate ownership, insufficient points, refunds
and successful acquisitions. Future-time and overflow states are rejected.

The persistent implementation authenticates the session, serializes account
updates and generates lottery draws in PostgreSQL. A browser cannot supply
winning choices or a seed. The source's posting-emote hook and
client script include are commented out, and its referenced
`april_process_post_emotes` implementation is absent from the archive. The
public emote endpoint remains closed by default. The missing posting contract
has not been reconstructed.

## Credential boundary

The optional account controllers use separate signed credentials. Cookies bind one event kind and run, expire after at
most 48 hours and cannot outlive the configured run. Key changes, altered
payloads, future issue times and noncanonical encodings invalidate them.

Same-address recovery uses the verified transport peer with a new, separately
derived HMAC key. IPv4 and its mapped IPv6 form identify the same peer. A
separate derivation produces the private storage capability; it is not the
cookie payload or a public poster/rate-limit identifier. No historical source
secret is reused. Runtime credentials stay in HttpOnly, SameSite=Strict cookies, with Secure and
the __Host prefix in production. Storage capabilities stay out of public JSON
and templates. Each operation checks current activation before account access.
Signing keys alone cannot enable an event.


## Optional local runs

`PUBLIC_EVENT_WORKFLOWS` is unset/false by default. Only the literal `true`
enables the reconstructed controllers and native pages; it also requires
`POSTER_ID_KEY`. Invalid flag values stop startup. Enabling the flag does not
create a run or account. With no active run, the historical endpoint responses
remain unchanged.

Migration 0251 adds a private event schema and a NOLOGIN function owner. On an
existing installation, the database bootstrap administrator must create that
role before the migrator applies 0251:

```sql
CREATE ROLE board_event_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
  NOREPLICATION NOBYPASSRLS;
GRANT board_event_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
```

Fresh installations use `deploy/roles.sql`. Do not rerun that entire bootstrap
file on an existing installation. Public credentials cannot read private
accounts, write quotes, enable runs or assume the event owner role. Startup and
readiness checks reject changed authority when event workflows are enabled.

Build `board-events` from `board-store`. Its `validate` command is offline;
`start`, `quote` and `close` require `MIGRATION_DATABASE_URL` with the actual
`board_migrator` login. A privileged login that merely assumes that role is
rejected.

```sh
cargo build -p board-store --bin board-events --locked
./target/debug/board-events validate event.json
./target/debug/board-events start event.json
./target/debug/board-events quote quotes.json
./target/debug/board-events close 1
```

A profile contains `version: 1`, `kind: "market"` or `"emotes"`, integer Unix
seconds `starts_at` and `ends_at`, and `account_capacity` from 1 to 100,000.
The end must follow the start and cannot already have passed. Market profiles
also require a `prices` object with positive integer quotes for all 18 symbols:
PEPE, WOJK, ANIME, CHAD, CLOWN, LOL, SICP, AUTSM, BANE, CIA, BOOB, RDDT, DESU,
JANNY, GME, CHUCK, YTSB and GACHI. Emote profiles omit prices. Quote updates
contain `version: 1`, the returned `run_id`, and the complete `prices` object.
Inputs are limited to 16 KiB; duplicate quote keys and unknown fields fail
validation.

Only one run of each kind can be enabled. Close the previous run before
starting another. Closing preserves accounts; a new run gets a new ID and
fresh credentials, so old cookies cannot carry balances into it. Expired runs
can be replaced without deleting their history. The tool refuses new runs at
10,000 retained runs rather than silently discarding records. Closing races
against account writes on the same run lock, preventing a queued operation
from committing after closure wins that lock.

When enabled, the homepage links to `/event-market.html` and
`/event-emotes.html`. These pages use ordinary forms and work without JavaScript.
Successful writes redirect to GET; refreshing a result cannot repeat a trade
or purchase. Reads do not open accounts. Roll results shown in the redirect
query are presentation only and must name choices the current account owns.
The pages reconstruct missing UI; they are not claimed as recovered historical
screenshots. Unicode emotes render, while unavailable image artwork is named
and identified as missing.

The source JSON shapes remain on `/xa24tb.php` and `/emotes_xa22.php` when a run
is explicitly enabled. The server derives same-peer recovery from verified
transport information, not forwarded headers supplied by a visitor. Emote
starting credit is 100 only for an existing verified anonymous session whose
source network-mask age is at least one hour; otherwise it is zero. Native
pages display elapsed credit, while source JSON retains the stored balance.

## Workflow checks

Local database tests cover account caps, same-run ownership, simultaneous
trades, duplicate purchases, lottery credit/refunds, quote updates, closure
while a mutation waits, reset and runtime privilege refusals. HTTP tests cover
closed defaults, signed-cookie recovery, forged/duplicate cookies, bounded
forms, origin refusal and native page rendering. Configuration and operator
input tests reject implicit activation and malformed profiles.

`scripts/test-optional-events-migration.sh` uses its own disposable PostgreSQL
cluster. It checks populated predecessor history, rollback, rejected authority
drift, retained event accounts and administrator dump/restore. It compares the
catalog definition before and after restoration without replacing its stored
fingerprint. Ordinary Pass, VIP, challenge and report readiness must remain
valid.

CI also runs `tests/browser/events-native.mjs` against a real public router and
private database accounts. It exercises successful native purchases/rolls and
refresh behavior with JavaScript disabled/enabled at desktop/mobile sizes.
These browser cases require a passing CI result; local HTML tests alone do not
qualify them. Scoring/perk workflows, seasonal presentation and missing artwork
remain part of issue #224.
