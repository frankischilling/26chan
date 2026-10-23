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
  it is not the production JSON contract. The rewrite preserves these 18
  stored values in desktop/mobile headers and native snapshot, quote and search
  recipes. Ordinary public writes cannot choose an identity value. Badge artwork
  and its original event styling are still missing; matching markup is not a
  claim of visual parity.

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
Optional market, emote, score and perk behavior are implemented below.
Seasonal presentation, missing artwork and final browser qualification remain open.

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

A profile contains `version: 1`, `kind: "market"`, `"emotes"` or `"scores"`, integer Unix
seconds `starts_at` and `ends_at`, and `account_capacity` from 1 to 100,000.
The end must follow the start and cannot already have passed. Market profiles
also require a `prices` object with positive integer quotes for all 18 symbols:
PEPE, WOJK, ANIME, CHAD, CLOWN, LOL, SICP, AUTSM, BANE, CIA, BOOB, RDDT, DESU,
JANNY, GME, CHUCK, YTSB and GACHI. Emote and score profiles omit prices. Quote updates
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
qualify them. Score and perk browser qualification, seasonal presentation and
missing artwork remain part of issue #224.

## Dormant scores and perk glyphs

The scoring corpus executes 88 isolated cases through the original controller
and post-score hook on an owned ledger. It records self-like refusal by peer
or Pass identity, duplicate precedence, the exact 120-second boundary, range
refusal, challenge decisions and the counter ceiling. The five excluded boards
(b, qa, s4s, bant and vip) still count likes but award neither score; their
post hook also awards nothing. Other boards award 2 for a post, 1 for giving a
like and 10 to its recipient. The public getter still returns zero.

Some source failures leave a vote-log row before rejecting a changed target
or full counter. The Rust model returns a complete outcome; the private SQL ledger commits
the vote, counter and both awards atomically. It does not reproduce partial
failed writes. Synthetic target changes between the source's two reads are
included in the corpus; no historical posts or identities are used.

A second corpus executes all 231 glyph entries across the 19 active perks.
Random pools retain duplicate entries and their source order. Negative scores
unlock none; zero unlocks every active choice. Commented image/animation perks
stay unavailable even at a higher score. Glyphs come only from pinned numeric
Unicode entities.

With event workflows enabled and an active score run, the ordinary Options
field accepts the first recognized perk name. The server chooses from its
original pool and saves a finite ordinal, not HTML. The posting trigger checks
the active run under lock after verification. Expired or closed runs leave the
post undecorated; disabled deployments ignore these choices.

Saved glyphs render after the comment in the source's `like-perk-cnt` wrapper.
The main JSON projection retains the original numeric entities. Native updater,
quote and search recipes accept only the 214 distinct glyph strings represented
by the 231 pool entries. Catalog teasers and server search include the selected
glyph. Authenticated thread transfer preserves it, and whole-post erasure
removes it.

`/event-perks.html` lists the 19 choices without creating an account or cookie.
Its read-only board/post form opens the native like page; it never submits a
vote. The page is absent when workflows are disabled and shows a closed message
without an active score run. This is an explicit reconstruction of the dormant
helper, not evidence that the supplied PHP posting hook was enabled.

The browser regression for saved comment readers includes all 231 perk choices.
It must pass in CI; typed rendering and local parser tests alone do not qualify
actual browser cloning or display.

## Optional score ledger and native form

Score runs use the same explicit operator activation and retained-run limits as
other events. The public service also needs its private verifier configured
with `PUBLIC_VERIFICATION_ADDR` and `PUBLIC_VERIFICATION_TOKEN`.
`/event-like/<board>/<post>` provides a native, script-independent
form for a live reply. A successful vote redirects to a read-only count page.
Opening a result page never votes or creates a score account. Opening posts,
archived threads and deleted targets cannot receive likes.

When verification is required, the mutation consumes a distinct `like` proof
bound to its board, post,
anonymous session, peer and run-derived form nonce. Posting/reporting proofs
cannot authorize a like, and a like proof cannot authorize those actions.
Closure rejects outstanding proofs. The native form checks the browser's
HttpOnly CSRF secret and current run before contacting the issuer. It does not
use the historical `xa19` echo cookie as authorization or accept proof hashes
from browser fields. An address with an earlier accepted anonymous vote can
skip CAPTCHA. Source
authentication-cookie presence or a native verified-email session requires
verification again. Advice is read-only; the mutation rechecks private history
and the live browser session under the run lock. A Pass vote alone does not
make the anonymous address eligible. The exemption retains CSRF, cooldown,
range, self-like and duplicate checks.

SQL derives the voting identity from a checked Pass receipt or the private
posting-address fingerprint. Self-voting is rejected by both identity and
address. Only global IPv4 range rules apply to anonymous votes, matching the
source call's empty scope/options; Pass bypass is decided by the consumed
receipt. The supplied caller omits the range helper's required ASN argument.
The reconstruction uses its intended IPv4-only lookup rather than reproducing
a PHP argument error. Network policy stays locked through the mutation.

The score SQL owner cannot select stored posting-address hashes. The history
owner returns only a run-scoped pseudonym, a self-match result and the posting
transaction check. Network helpers expose a lock operation and a boolean
range result. Automatic-session and reporting permission guards retain their
original boundaries. Public readiness is tested with events both off and on,
including after upgrade and restore.

Accounts use run-separated private hashes. Event tables contain no raw address,
Pass identifier or login credential. Each run has the configured account cap
and at most one million accepted votes. Reaching a cap rejects the entire
mutation, including receipt consumption. Vote history remains with the closed
run so deletion cannot erase duplicate or cooldown evidence. A reset starts a
new run; it does not rewrite the old scores.

The posting hook adds two points in the accepted-post transaction, after its
private identity has been captured. Both an active score run and the public
service's explicit event-workflow setting are required. Ordinary callers and
historical imports remain unscored. The five excluded boards retain their
source zero-award behavior. Operator closure takes its table gate before a
runtime row lock can create a counter-update deadlock.

Owned database tests cover cross-action proofs, one-use consumption, known-address
exemption and its authentication-cookie override, ordinary and Pass votes, self/duplicate refusal, exact cooldown boundaries, range
bypass, capacity rollback and the posting opt-in. The upgrade fixture retains
score accounts, vote history and counters across administrator dump/restore.
Native HTTP tests use the real private verifier; real-browser qualification of
the score form is still required before claiming frontend parity.
