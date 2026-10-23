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
The two closed endpoints have local handlers that do not consult a database,
read event cookies or create accounts. Eighteen independently executed source
action cases are replayed through GET, HEAD and same-origin POST. Cross-origin
writes retain the common refusal. The emote error uses plain text rather than
PHP's implicit HTML content type; response text and status are unchanged.

The isolated helper corpus covers 94 stock-identity, investor-name, Halloween
badge and perk-option cases. It executes pinned slices with PHP 8.3; the
historical production PHP version is unknown. Stock valuation is supplied as
a scalar, and the score-read boundary retains the source's unconditional zero.
These tests do not establish an active scoring, market or emote workflow.
Optional activation, persistent state and browser qualification remain open.

## Dormant market model

The pure Rust model preserves the 18 symbols, 1,000-point starting balance,
1,000-unit order ceiling, ten-second cooldown, stale-price refusal and public
balance projection. A separate corpus executes 108 cases through the original
buy/sell functions against an owned in-memory ledger. It covers scalar coercion,
validation order, zero holdings, insufficient funds and exact cooldown edges.
No historical account or quote data is used.

The model returns a complete next account rather than changing half a trade.
Overflow and unsupported symbols are rejected. A future persistent controller
must authenticate the actor and lock the account and quote in one transaction;
the pure arithmetic does not provide those guarantees by itself. The HTTP
endpoint remains closed. None of this code connects to financial services or
handles real money.

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

A persistent implementation must authenticate the session, serialize each
account update and generate every random draw on the server. Callers must not
accept lottery results from a browser. The source's posting-emote hook and
client script include are commented out, and its referenced
`april_process_post_emotes` implementation is absent from the archive. The
public emote endpoint remains closed; storage, activation and that missing
posting contract have not been reconstructed yet.

## Credential boundary

The optional account models have a separate credential primitive, not yet wired
to the closed endpoints. Cookies bind one event kind and run, expire after at
most 48 hours and cannot outlive the configured run. Key changes, altered
payloads, future issue times and noncanonical encodings invalidate them.

Same-address recovery uses the verified transport peer with a new, separately
derived HMAC key. IPv4 and its mapped IPv6 form identify the same peer. A
separate derivation produces the private storage capability; it is not the
cookie payload or a public poster/rate-limit identifier. No historical source
secret is reused. Runtime code must keep credentials in HttpOnly cookies,
keep storage capabilities out of responses and logs, and check current operator
activation before every operation. These primitives do not grant persistent
database access or enable an event by themselves.
