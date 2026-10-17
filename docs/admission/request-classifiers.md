# Request classifiers

Classifier coverage is incomplete. The request-header credit predicate is
connected to public posting through migration 0193. Public posting now applies
the native numeric and country classifiers. Reporting captures a typed native
threshold observation without changing weights. Trusted edge automation and
complete historical behavior still need the inputs described below.

## Credit eligibility

The source's `spam_filter_is_bad_actor` uses header presence, case-sensitive
user-agent markers and two referer substrings. Empty header values count as
present. The native predicate accepts qualified byte values or an unavailable
provider and limits inspected values to 4,096 bytes. A definite independent
match can still withhold credits when another provider is unavailable. A
missing required fact cannot qualify the request as eligible.

The active caller at `imgboard.php:4959–4961` uses this predicate only after
successful CAPTCHA validation, alongside its unsolved-count and operator-policy
checks. It does not independently reject posts or turn a failed CAPTCHA into a
success. The similarly named CAPTCHA difficulty checks have a separate caller
and belong to the CAPTCHA workflow.

The extractor runs the pinned pure function on PHP 8.4.24 over 450 synthetic
requests, with a fresh static cache for each case. It never loads the
application, database or RPC files. Cases distinguish absent and empty headers, marker case, embedded
NUL bytes and non-ASCII user agents.

The public caller derives facts from the posting request. Accept and language
use presence alone; repeated values still prove presence. Repeated user-agent
or referer values are unavailable when their contents matter. Required unknowns
withhold courtesy credits without rejecting a valid solved-CAPTCHA post.

The proof carrier defaults to withholding credits. Its request eligibility is
installed in a separate transaction-local setting and cleared by the existing
private receipt consumer. The private issuer's permission, unsolved count,
challenge validation and registered-activity requirements still apply. The
request predicate cannot authorize a challenge or replenish credits spent by
a credit-backed receipt. Report proof serialization stays unchanged.

The database cases cover eligible, withheld and unavailable request facts,
accepted posts without credits, one-use receipts, late attachment rollback and
credit-spending rollback. Migration qualification retains pending challenges,
receipts, existing balances and anonymous sessions through rollback and upgrade;
the migration pins the predecessor owner, path, grants and body. The upgrade
exercise checks path, grant and body changes.

### Numeric score and courtesy credits

The source's additional `<0.15` check does not require a new score provider for
courtesy credits. Each granting tier requires `isUserKnown` for at least three
days. On the same state and clock, that implies the six-hour
`isUserKnownOrVerified(360)` check. The posting caller leaves its score at zero
for those users, so every otherwise eligible grant already satisfies the
numeric condition.

This depends on call order. The source grants credits before
`updatePostActivity`; the native writer calls `finish_post_challenge` before
advancing anonymous activity. Both use the pre-post session state. Idle reset
clears eligibility rather than creating it. Verification alone and six-hour
knownness alone do not authorize a credit.

Boundary tests exercise all credit tiers, pending activity, churn, verification
and resumed state. A separate database test compares 1,260 actual private credit
profiles with domain results and requires every positive profile to satisfy the
six-hour predicate. The separate header predicate, issuer permission, board
policy, post-count requirements and receipt rules remain in force. No score is
fabricated or persisted, and this implication does not exempt unknown users
from later posting classifiers.

Source: `imgboard.php:6251–6259,6571–6586`, `lib/captcha.php:155–197` and
`lib/userpwd.php:487–547`. Changing a credit tier, known-user predicate or the
state/clock ordering requires requalifying this implication.

## Optional automation evidence

The domain automation predicate matches 429 source score/cache/user-agent cases.
It preserves the source's default threshold, WebView exclusions and early return
when the edge score is absent, even if a cache marker exists. A definite low
score does not need a cache lookup, while missing required evidence remains
unavailable. No client-supplied header has been wired as a trusted score.

## Numeric request score

`content_admission::threat` evaluates the source's deterministic numeric rules
against a qualified request snapshot. It takes the complete ordered server
entries, parsed cookies, challenge/response values, request time, country and
posting/reporting mode. A normal unordered HTTP header map cannot establish
this snapshot. The caller must qualify transport order and trusted country/time
facts before supplying it; client headers cannot assert that provenance.

The domain model preserves byte matching, empty-versus-absent fields, PHP's
falsey `"0"`, browser version branches, cookie timing, all 224 country-language
mappings and ordered multiplication. The Linux platform branch retains the
source's regex-error comparison; an ordinary non-match adds no penalty. Server
entry order includes non-HTTP entries for the separated-header and model-distance
checks. Parsed cookies and the raw Cookie header remain independent facts.

Snapshots are bounded to 256 unique entries per map, 128-byte ASCII keys,
4,096-byte values and 65,536 total key/value bytes per map. Duplicate keys,
oversized inputs and unavailable provenance produce no score. Time subtraction
accepts finite decimal/exponent strings with surrounding ASCII whitespace;
unqualified or unsupported numeric inputs remain unavailable. Request time is
bounded to the exact integer range of a double. No score means unknown, not zero.

The isolated extractor executes only the two hash-pinned pure functions on
64-bit PHP 8.4.24 with PCRE 10.46. It rejects other engine versions. Its 4,524
cases cover browser/version branches, request mode, header presence and order,
cookie timing, language boundaries, every country-language
alternative and combined terms. Arbitrary-byte tests also check finite results
and bounds. This corpus qualifies the model; the production native profile and
posting/reporting callers are described below. The native profile defines the
replacement server-entry layout. The older report-threshold model still provides
only its selected positive witnesses.

Regenerate or verify the corpus without loading application code:

```text
php -n scripts/extract-threat-score-reference.php SOURCE crates/domain/tests/fixtures/threat-score.json --check
```

## HTTP observations

`board_http::request_facts` captures CGI-shaped `HTTP_*` fields in the actual
HTTP/1 parser's order. It checks that the ordered names and current header map
still describe exactly the same unique fields. Missing parser metadata,
repeated fields, underscore/hyphen collisions, oversized values and changed
header membership are unavailable. It never uses `HeaderMap` iteration as a
wire-order fallback. Names-only metadata cannot detect value-only middleware
changes; capture must happen before such mutations. Values are observations
of the current request, not independent proof of original wire values.

Raw Cookie headers remain separate from parsed cookie observations. The
accepted subset contains flat ASCII names and bounded byte values, with optional
single spaces after semicolons. It decodes valid percent escapes once and retains
literal plus signs, matching PHP cookie parsing rather than form decoding.
Mixed digit-leading names such as `4chan_name` are supported. Quoted values,
array or normalized names, all-numeric names, duplicates, malformed escapes and
other unsupported syntax leave the parsed-cookie provider unavailable. They do
not erase the raw header. Empty
and absent Cookie headers both yield an empty parsed map but remain distinct
HTTP observations.

The cookie fixture uses an owned loopback PHP server with no application code.
Its 109 HTTP cases cover 91 accepted cases and 18 deliberately unsupported forms
on 64-bit PHP 8.4.24 / PCRE 10.46. This qualifies the chosen subset against that
SAPI, not every historical deployment. Actual native listener tests cover
capture off/on, field order, empty and encoded cookies, repeated headers
and name collisions. Captured values have no debug or serialization interface;
callers must not log or persist the raw fields.

The PHP fixture qualifies the cookie helper's raw input subset. Native HTTP
parsing trims trailing field whitespace before capture; for example, a wire
`Cookie: a=1 ` becomes the current header value `a=1`. A TCP control covers that
normalization. It differs from the PHP probe's retained trailing space and is
not evidence of historical SAPI wire-value equivalence.

The supported values include the application's percent-encoded name and options
preferences, UTF-8, literal plus, encoded delimiters, NUL and arbitrary escaped
bytes. Fixture values use hex so invalid UTF-8 does not disappear during JSON
serialization. Bounds apply before copying or decoding. These observations still
confer no session or posting authority.

This capture adapter does not supply country, time, session history or form
fields. The native profile below combines it with explicitly supplied context;
application posting and reporting decisions are not connected yet. Public
posting, direct reports and legacy categorical reports retain absent
versus present-empty challenge and response fields through their parsed form
inputs. Verification still treats missing and empty text alike and obtains
authority only from the private issuer. Seven executed source cases distinguish
missing, empty, zero and noop challenge/response values at the timing rule. The source
sets `_tcs` only in its framed CAPTCHA response; the JSON path does not set it.
The native JSON CAPTCHA client does not synthesize the cookie.

```text
python3 scripts/extract-flat-cookie-reference.py PHP crates/http/tests/fixtures/flat-cookies.json --check
```

## Native request profile

`board_http::request_facts::native_profile` defines the replacement request
layout. Its server-entry list contains the parser-ordered `HTTP_*` fields, with
no non-HTTP fields interleaved. Request time is supplied separately. This is an
explicit infrastructure choice: the source files do not establish the original
PHP SAPI's server-entry order. Interleaved entries can change its SEC_FETCH
contiguity and model/platform-distance rules. The profile must not be described
as a reconstruction of that historical deployment.

Posting uses the source's multipart mode and the actual OP/reply choice, with a
qualified two-letter country from verified-peer lookup. Reporting uses OP=true,
multipart=false and no country. Time must come from the server's request-start
observation. Header values named for geography or request time do not supply
these facts. Challenge and response retain absent-versus-empty values.
Unavailable capture, parsed cookies, time or needed country yields no score.

The native Referer check compares a parsed HTTP(S) URL with the configured public
origin, including its scheme and port. Credentials, foreign hosts and malformed
values do not match. An absent Referer keeps the source's absence behavior.
This replaces the source's literal `4chan.org` substring check so a local or
separately hosted board can use its own origin. The original scoring entry point
retains the literal rule for source qualification. Neither path rewrites the
observed header list, and this scoring condition grants no CSRF or session
authority.

The profile matches 2,797 applicable cases from the independently executed PHP
corpus: cases with HTTP-only server entries, matching canonical parsed cookies,
actual caller modes and the same Referer verdict. Other fixture cases are not
counted as native-profile matches. Separate controls cover unavailable evidence,
header order, configured-origin differences and unchanged raw fields. Actual TCP
listener cases exercise capture enabled and disabled, source posting/report
vectors, repeated fields and synthetic server-owned context with sensitivity
controls. Those test handlers do not qualify production geography or clock
wiring. Historical SAPI order and reverse-proxy rewriting remain outside that
evidence.

A caller must distinguish an explicitly disabled optional classifier from
missing evidence required by an enabled policy. Missing needed evidence must
fail closed; it cannot skip rejection or become score zero. An optional credit
bonus can instead be withheld. Public posting consumes this native profile in
its late decision path. The reporting decision caller is still separate work.

## Late posting decision model

`content_admission::classifiers::posting` covers the ordered source checks from
numeric-score initialization through the final timing-cookie refusal, before
the disabled flood call and optional edge/history checks. It distinguishes a
six-hour known-user score bypass from unavailable scoring evidence. Knownness,
trusted post counts and verification come from the qualified pre-post anonymous
state. The caller must use the same frozen time that resumed that snapshot;
a later wall-clock read changes new-user and mask-change comparisons. Board
validation remains a prerequisite. A cosmetic badge cannot supply the CAPTCHA
bypass input.

The model preserves the strict `>0.31` early refusal, the later `>=0.09` checks,
board/thread/file conditions, mask-change event ordering and random thresholds.
It requests draws only where the source does. In particular, the public-proxy
condition draws before testing the `/f/` exception. Missing or out-of-range
randomness is unavailable, never a favorable default.

An isolated harness runs the hash-pinned source prefix and selected pure
`UserPwd` methods. It substitutes the invocation's fixed board identifier and
records logging, response and random calls through local stubs. It never loads
the application or connects to storage. Its 13,280 cases compare outcomes,
ordered event names and exact draw ranges, including combined markers, pending
activity, new-user mask suppression, known/trusted/verified distinctions and disabled source branches.
These cases do not qualify logging payloads or database effects. The native
draw adapter uses the same bounded rejection-sampling helper as dice and
fortunes, driven by system entropy. It accepts only the two source ranges and
returns unavailable on entropy failure or invalid output. It preserves uniform
threshold probabilities, not PHP global PRNG seeds or cross-request sequences.

Thread text, browser-ID, country and timing-cookie providers are checked only
when reached. Input bounds are stricter than this source prefix; exceeding them
is an intentional fail-closed case. Draw ranges follow the source. An unavailable needed fact
returns no decision or partial event list. The store applies the returned plan
inside the posting transaction. The hardcoded browser-ID condition
also needs a qualified provider in the chosen fingerprint namespace; the model
alone cannot establish equivalence between operator keys and the old deployment.

```text
php -n -d extension=tokenizer scripts/extract-late-post-reference.php SOURCE crates/domain/tests/fixtures/late-post.json --check
```

## Late history checks

The separate history model covers the following edge and country checks in
`imgboard.php:6394–6449`. It retains the source's disabled flood call. A trusted
edge field must be present before checking the same-session bot cooldown; an
existing cooldown takes precedence over the automation predicate. Verified or
24-hour-known actors skip that edge stage. Country checks still apply to
unverified actors with fewer than 20 posts, including pending post activity.
An existing country-change event takes precedence over a new environment change.

The isolated source harness exercises 12,416 cases around the one-day age and
20-post boundaries, verified actors, new users, bypasses, absent and zero-valued
edge fields, and both histories. Tests compare refusal and event order. Providers
that the source never reaches are deliberately unavailable in the Rust replay.
The returned events distinguish private history writes from diagnostic log
entries; neither is persisted by this pure model.

History results must come from the same private session and the source's strict
24-hour cutoff. The fixture supplies lookup results, so it does not qualify SQL,
clock selection, retention, or concurrent event writes. Failed lookups remain
unavailable rather than inheriting the source's false-on-database-error behavior.
The public posting writer uses the country-history bridge below and revalidates
its authority before committing a refusal. The optional edge producer still
needs a trusted ingress adapter.

```text
php -n -d extension=tokenizer scripts/extract-post-history-reference.php SOURCE crates/domain/tests/fixtures/post-history.json --check
```

### Private lookup storage

Migration 0197 adds the private lookup projection for `block_bm_bot` and
`country_changed`. Each row contains only a hashed anonymous capability, a fixed
kind and the last event's integer timestamp. The composite key and two-kind
constraint bound storage to two rows per existing capped anonymous session.
Deleting a session cascades its rows; deleting a post or transferring a thread
does not move this session-wide history. Stale rows may remain until session
removal. The 24-hour limit controls matching, not physical retention.

Private helpers lock the session and use the database's current second after
waiting. Matching uses only the strict lower cutoff, as in the source; it does
not invent an upper cutoff for future timestamps. Repeated active events do not
extend the cooldown. A recorder can refresh an expired event, but cannot create
a session, change activity counters or choose its own event timestamp.

The dedicated catalog attests the table shape, constraints, indexes, grants,
schema permissions, function definitions and foreign-key trigger state. Creation
removes default grants to other roles before applying the narrow private grants. The transfer and restriction
catalogs remain separate and unchanged. No runtime role can read the table or
execute these helpers. This migration does not enable posting enforcement or
provide the future authenticated refusal issuer.

Database tests cover distinct kinds and sessions, cutoff boundaries, unchanged
activity, repeat events, missing/expired sessions, runtime denial, catalog drift
and serialization of concurrent same-session issuers. Upgrade qualification
checks rollback, retained session state, predecessor drift, role-specific default
privileges and a full dump/restore
with populated private history.

### Country refusal bridge

Migration 0198 adds a public-runtime lookup bridge and a country-refusal issuer.
The issuer locks the public board and requires CAPTCHA to be enabled. It then
locks an established anonymous session, resumes the server-derived fingerprints,
and checks the source predicate: unverified, fewer than 20 posts including the
pending POST bit, and either active country history or an environment timestamp
matching the same frozen request second on a non-new session. A fresh session
cannot manufacture a country refusal. Unjustified attempts raise and roll back
the provisional refresh.

An older request cannot overwrite newer committed session clocks or fingerprints.
That fail-closed rule is an intentional backend concurrency safeguard. Event
timestamps still come from the database, and an active event is not extended.
The issuer does not accept a client-selected event kind or trusted-edge score.
Its seal covers the consumed session tables, resume helper, board RLS policies,
wrapper definitions, schema permissions and private-owner role posture.

The public store recorder owns a fresh transaction and returns success only
after commit. Its caller must first discard any provisional post transaction.
The store also has an internal form for a retained posting transaction: after
rolling back provisional content and replaying earlier originality effects, it
records the country refusal without committing. The enclosing commit makes
both effects durable together; rollback removes both. Only a confirmed commit
permits a refreshed session cookie in the refusal response.

The lookup returns observations, so the caller must still apply the locked
CAPTCHA/staff bypass decision before using them. The public posting writer now
uses the country bridge and the bounded recorder described in
[classifier diagnostics](classifier-diagnostics.md).

The late history gate uses trusted edge-field presence independently of numeric
parsing. Empty and non-numeric values still enter the source history branch;
only confirmed absence skips it. The executed corpus includes both forms. This
does not grant authority to a public request header or qualify numeric coercion
inside the separate automation predicate.

### Originality state before a late refusal

The source runs Robot9000 before the late classifiers and before inserting the
post. A successful originality check can save a text digest or decay an expired
mute. Those effects survive a later quiet or range refusal. The native writer
currently performs the check after provisional content writes so that posting
and attachment guards have already run.

The store keeps the checked board, actor, prepared text and timestamp together.
After rolling back provisional content, replay uses those exact inputs and must
return the same decision; otherwise the caller discards the transaction. The
Robot9000 rejection path and late-classifier refusal path both use this helper.
A country refusal and its diagnostic events commit with the replayed originality
state. An unsuccessful post does not advance anonymous posting activity.

Source order: `imgboard.php:6147–6157,6253–6450,6522–6556`; originality writes and
mute decay: `plugins/robot9000.php:161–222`.

## Public posting integration

The handler captures parser-ordered headers before moving the decoded challenge
fields. It uses the configured origin, server request time and server-derived
country, then retains only the qualified score, country and timing-cookie value.
These facts are bound to the board, parent, canonical peer and exact request
start. Missing or mismatched evidence stays unavailable; known actors still use
the source's explicit zero-score branch.

The writer reads CAPTCHA policy under the board lock and uses its pre-post
anonymous snapshot and frozen activity clock. Replies use the stored OP subject
and comment, rather than the reply text. The late checks run after the proved
post/attachment insert and successful Robot9000 check, before anonymous posting
activity and the final commit. Refusals roll back provisional content and media,
replay originality state with the same inputs, then commit only the applicable
country refusal and diagnostics. Quiet responses use the existing fake-success
number calculation. The backend may leave an unused sequence value after such
a rollback; allocated numbers are not recycled.

Only a committed country-refusal refresh permits a refreshed anonymous cookie.
A numeric quiet or image refusal does not refresh that state. This is a backend
lifecycle safeguard; it does not register a successful post. Receipt and media
consumption inside the discarded post are rolled back too.

Locked disabled-CAPTCHA policy and independently proved staff posting bypass the
late path. A solved challenge, selected badge or client cookie cannot grant a
bypass. Native posting currently has no trusted ingress-score adapter, so that
channel is explicitly absent; a direct `X-Bot-Score` header cannot activate it.

Store tests cover missing and mismatched evidence, a deterministic high-score
quiet refusal, a clean acceptance, first/repeated country refusal, disabled
CAPTCHA, receipt preservation, originality replay, diagnostic order and unchanged
posting activity on refusal. The receipt-credit fixture seeds an established
session's environment clock with its backdated creation clock; leaving that
clock at the current second would legitimately trigger the country predicate.

## Remaining source behavior

Reporting now captures a native-profile threshold observation at admission
(see [report request profiles](report-request-profile.md)). This is not full
historical PHP scoring or reason parity. Source-equivalent threat, staff and
history facts are still needed for that claim. Optional trusted-ingress bot scores and their
private event producer remain open. Validated Pass/local-auth CAPTCHA bypasses
also need their own native authority; neither is inferred from a cookie or policy
flag. The old hardcoded browser fingerprint still needs qualification in the
operator's fingerprint namespace. These gaps prevent claiming complete #212
parity despite the connected public numeric/country path.

The flood helper's posting call is commented out and the caller assigns zero.
Activating its dormant rejection branch would change the supplied behavior.
Disabled sampling and logging branches likewise must not become active filters.

Source evidence is in `lib/postfilter.php:1639–1682,1684–2316,2397–2545` and
`imgboard.php:4937–4969,6258–6449`. The remaining work stays under #212 and the
related CAPTCHA, session and restriction issues.
