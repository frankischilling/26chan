# Public Pass lifecycle

Issue #201 covers public membership authentication and its posting benefits.
The source state model is qualified. Private entitlement/session storage, the
isolated verifier and browser login/logout forms are implemented and undergoing
integration checks. The policy module alone grants no identity or posting
authority. CAPTCHA bypass is connected to protected posts; the remaining
benefits and recovery are still being implemented.

## Source decisions

`auth.php` applies a 15-minute login hold when the previous nonzero IPv4 /16
network differs. It checks this hold before the account status. Status 0
continues, 6 activates a new Pass, and 7 activates a delayed Pass with one year
starting at activation. Statuses 1 through 5 distinguish expired, refunded,
disputed, spam-revoked and illegal-content-revoked accounts.

`lib/auth.php:valid_captcha_bypass` uses different rules for posting. It first
rejects expiry at or before the request timestamp, then rejects any status
other than 0. Its network hold is 30 minutes with an IPv4 /24 comparison.
Same-scope use and an unset previous network bypass those holds. Future
last-used timestamps remain inside the hold rather than wrapping the clock.

Ordinary login cookies last 86,400 seconds. Remembered sessions use 31,556,900
seconds in the supplied login code. A trusted bypass halves reply and
duplicate-image cooldowns, rounding upward. Thread cooldowns remain unchanged.
The `since4pass` option exposes the original purchase year. Pass-only posting
uses the trusted bypass outcome, including the source's separately authenticated
staff path; it does not grant staff roles to a member.

The login source uses /16 and 900 seconds while posting uses /24 and 1,800
seconds. These are distinct contracts. The native implementation must not merge
them into a single timeout. Unknown stored statuses will fail closed rather than
fall through the PHP switch.

## Reference qualification

`fixtures/pass-policy-source.json` pins the two source files and contains only
the network/status decision fragments. Its PHP harness replaces cookie writes,
HTTP termination and activation SQL with recorded outcomes. It uses owned
scalar timestamps and documentation addresses, without loading the source
application, database adapters, keys or credentials.

The 1,120 cases cover all eight statuses, the different subnet boundaries,
unset prior networks, future timestamps, exact hold boundaries and posting
expiry. Rust compares each outcome with the executed source fragment. The
harness does not qualify credential verification, persistent updates, cookies,
recovery, payment callbacks or posting integration.

```sh
python3 scripts/extract-pass-policy-reference.py /path/to/4chan-old --check
php -n scripts/test-pass-policy-source.php
cargo test -p board-domain pass_policy --locked
```

## Integration boundaries

The public listener must not read membership credential hashes or update
entitlements directly. Authentication belongs to the isolated verifier, with
bounded password hashing and database-controlled login attempts. Browser
cookies carry opaque session material; network comparisons use trusted peer
information. Neither a presentation cookie nor a browser-supplied membership
flag can authorize a bypass.

Every protected write must recheck the current entitlement and session inside
the posting transaction. Issuing an advisory receipt must not let a revoked,
expired or signed-out session post later. The same write captures the private
membership identity for restrictions and staff history, while public output
receives only the explicitly requested visible membership fields.

Local provisioning must use deployment-owned test entitlements. Source tokens,
PINs, production salts and payment accounts are never imported. Recovery and
checkout dependencies remain part of the parent issue until they have working,
qualified boundaries.


## Current authentication boundary

The verifier reserves a failed-attempt slot before password work. Five failed
or unfinished attempts from a peer within an hour refuse further attempts;
correct credentials release their slot. Login completion consumes its one-use
30-second ticket and rechecks the credential version after acquiring the account
lock. PIN verification accepts only the fixed Argon2id profile and runs at most
two hashes concurrently. Missing identifiers use a dummy hash with that same
profile. Hash permits remain attached to blocking work after HTTP cancellation.

Sessions are random, separately hashed tokens with host-only HttpOnly cookies.
The source-compatible `pass_enabled` cookie is presentation only. Each account
has at most eight active stored sessions; global inventories are bounded.
Changing the PIN or account status advances the credential version and invalidates
older sessions. Login/logout forms have a separate CSRF cookie and nonce purpose.
Only the isolated verifier can call the issuer entry points; structural or
privilege drift blocks them.

IPv4 holds preserve the source /16 and /24 rules. IPv6 uses keyed /64 scopes
instead of collapsing every address through the source's IPv4 conversion.
Addresses and public login identifiers are HMAC-derived before database storage.
No runtime receives table access to credential hashes or membership sessions.

The supplied Pass template references an external stylesheet that is absent from
the checkout. A retrieval attempt returned an HTML site-unavailable page, which
was not installed as CSS. The local form stylesheet is provisional; complete
source-pixel qualification remains open. No production entitlement, payment or
mail account has been provisioned by these tests.


## Local operator provisioning

`board-pass-admin` reads one bounded JSON operation from standard input. It needs
`MIGRATION_DATABASE_URL` and the same protected `VERIFICATION_KEY` used by the
verifier. Keep that key stable and backed up: login identifier hashes cannot be
recomputed after losing it. The CLI refuses public/runtime database identities
and checks the sealed membership catalog before changing a row.

Use `cargo run -p board-verification --bin board-pass-admin --locked` with input
redirected from a private file outside the repository. The supported operations
are:

- `create`: identifier, PIN, status (0 active, 6 new, 7 delayed), purchase time,
  and expiry time. Delayed accounts omit expiry until first authentication.
- `reset_pin`: identifier and replacement PIN. Existing sessions are invalidated.
- `status`: identifier and status 0 through 5. A status change invalidates sessions.
- `renew`: identifier and a future expiry later than the stored expiry. It keeps
  the original purchase time and reactivates the account.

Dates use RFC 3339. Identifiers contain ten ASCII letters or digits and are
case-insensitive. PINs contain 1 to 128 bytes. Unknown fields and fields belonging
to another operation are refused. The result contains only the internal ID and
status; identifiers, PINs and hashes are never printed. Protect and remove any
input file containing credentials after use. These operator changes do not
contact a checkout or mail provider.

An eight-session inventory cannot lock out a user after a credential rotation:
login removes stale versions before applying the session cap. The SQL suite
checks this boundary. Provisioning tests use only a disposable database and
owned credentials, verify that IDs and purchase history are preserved, reject
shortening renewals and runtime-role writes, and inspect output for secret leaks.

Apply migration 0216 before running these binaries. The public client also checks
the explicit Pass readiness endpoint, so an older verifier cannot appear ready
for membership routes it does not implement.

The verification browser fixture includes Pass login, authenticated, completion
and error pages rendered by the production template. The browser cases cover
native form validation and POST fields on desktop and mobile, with JavaScript
both enabled and disabled. Submission is intercepted with owned fixture data;
these cases do not prove session issuance or posting benefits. Private issuer
and full-router database tests cover login and revocation separately. Hosted
browser results must pass before this work is ready to merge.

Posting advice uses the source's 30-minute network hold and checks membership
expiry before that hold. Login still uses its separate 15-minute, wider-network
rule. The internal advice endpoint returns only a state and, when valid, a
purchase year. It does not change the last-used network or issue a receipt.
A protected write must recheck membership under locks before applying benefits.
The SQL and private HTTP tests cover differing IPv4 /24 scopes, the release
boundary, absent previous scope, expiry precedence and signed-out sessions.

The private issuer can now create 30-second membership receipts bound to the
anonymous session, peer, form, board and write purpose. It limits outstanding
receipts to eight per Pass session and a configured global inventory. Private
consumption rechecks the current session, credential version, expiry and network
hold under write locks. Signout removes outstanding receipts; a failed write
rolls consumption back. SQL tests cover wrong context, replay, expiry, rotation,
capacity and rollback. HTTP tests check the issuer response and reject email or
newly minted browser contexts. A two-connection test observes the duplicate
consumer waiting on a database lock, then rejects it after the first commits.

Thread and reply forms now obtain these receipts for authenticated Pass users.
Classic, Twister and script-disabled controls keep the form nonce while hiding
the CAPTCHA. The post insert consumes the receipt, and a private transaction-only
handoff supplies the source's late-classifier bypass. Commit removes that
handoff; it cannot authorize a later request. Pass posts do not earn courtesy
CAPTCHA credits. Invalid, expired, disabled and network-held membership
responses clear both browser Pass cookies. Issuer outages leave them intact.
Store tests cover a real accepted post, missing proof, replay
and revocation; public-handler tests cover the private issuer and form advice.

The board policy follows `config/boards/vip.config.ini`: `/vip/` is Pass-only,
while other boards default to ordinary admission. With CAPTCHA enabled, the
post trigger requires a consumed Pass receipt; a solved ordinary CAPTCHA cannot
satisfy the membership gate. The original `valid_captcha_bypass` treats
CAPTCHA-disabled boards as bypassed, so that configuration also permits ordinary
posts on a Pass-only board. The separate staff authority remains unchanged.

Ordinary text and image replies now use ceil-half cooldowns for a current Pass
receipt. Preflight holds the receipt, session and account locks without consuming
the receipt; the eventual post insert repeats the same validation and consumes
it. A refused cooldown can retry with the still-valid receipt. Thread delays are
unchanged. CAPTCHA-disabled boards also receive the source bypass reduction.
The ordinary full-delay SQL entry point remains available for callers that
explicitly need ordinary timing; protected public writes use the new checked
entry point. The public role cannot call the core function with its own discount
flag.

The exact `since4pass` option now publishes the purchase year from the consumed
membership receipt. The source's sage removal runs first; the option remains
case-sensitive and ASCII-space-delimited. Ordinary posts and members who did not
opt in store zero. JSON omits zero and values at or above 10000, without exposing
an account ID. Thread transfers preserve the year and bind it into their source
fingerprint. Whole deletion clears it; archiving retains it.

The badge bitmap, `minileaf.gif`, is absent from the supplied archive. Its host
returned an unavailable page rather than an image. Public badge rendering still
needs that asset and visual qualification; no substitute image is shipped.

Private post associations now capture the login identifier's HMAC when a Pass
receipt is consumed. PIN changes preserve that identity; public, staff and
verifier runtime roles cannot read the association table. A failed insert rolls
back receipt consumption and the association together. The qualification suite
rejects an oversized name after receipt consumption, then successfully retries
with the same receipt.

Thread transfers bind the private identity into their source fingerprint, copy
it to the destination posts, and erase the source association. Deleting a post
or archiving its thread removes the private identity. The opt-in public year
still survives archiving. Tests cover transfer drift, erasure, disabled cleanup
triggers, privilege drift and a populated database restore. Posting bans now use these associations; report matching still needs integration.

Public image replies now check the source's same-image timer before the ordinary
reply timer. The comparison uses the approved normalized upload digest, the same
board and posting identity, and the newest matching post number. Source OPs have
`resto=0`, so an OP image can match a reply in its own thread. File-only deletion
retains the timer; whole deletion and archiving remove the posting history.

The interval is 300 seconds for ordinary posters and 150 for a checked Pass
receipt or CAPTCHA-disabled board. A receipt preflight does not consume the
proof. The wrapper revalidates the upload capability, and the public role cannot
call the core with its own discount flag. Twenty-two timing cases execute the
isolated PHP fragment. Store qualification also covers exact boundaries,
revocation, upload reuse after refusal, digest/scope separation and file-only
deletion. Legacy approved uploads with no normalized digest remain unknown;
the check does not infer one from the original input or a later asset.

Authenticated janitor replies use the same half-delay through a staff-only
wrapper after their posting authority is issued. Other staff modes retain their
separate timer policy.

Posting restrictions now match private membership identity after the source's
password branch. A valid membership skips host matches; template 124 is skipped
only in the membership branch, after warnings and expiry. The early decision
validates the receipt without consuming it. The final insert checks the current
transaction's consumed handoff, so a retained historical post cannot authorize a
new write. Refused inserts restore their receipt and leave no post association.

Staff bans capture the source post's membership identity. Janitor requests
capture it before removing the source post, and approval copies that retained
identity into the ban. Queue summaries and the scoped history reader include
membership-linked bans from other addresses without double-counting the same
row. Detailed history still enforces board access and never returns the private
membership hash. Automatic file bans
revalidate the same receipt in the separate refusal transaction and retain that
identity too. PIN changes and address changes do not evade those matches.
Network and automatic-range checks use a separately checked membership boolean;
disabling CAPTCHA alone does not grant that network exception. Public runtime
roles cannot read membership hashes or choose a trusted ban identity.

The reconstructed `/banned` controller now includes membership-linked warnings
and bans after an address change. Status reads and warning acknowledgments use
separate thirty-second receipts, bound to the anonymous session, board, form
nonce and exact action. Viewing status or a warning leaves the posting-network
hold unchanged. A status receipt cannot acknowledge a warning or post;
a warning receipt cannot acknowledge another ID. Consumption repeats account
expiry and credential-version checks, and replay fails.

The public page keeps its separate HttpOnly CSRF cookie and private, no-store
response. Only public restriction reasons leave the private reader. The existing
address and anonymous-identity status behavior remains available. The original
archive has no `/banned` controller, so this extends the documented replacement
contract rather than claiming an exact rendering match. SQL qualification covers
wrong-purpose and wrong-target proofs, another member's warning, revoked
credentials, private-function grants and a populated restore. The private issuer and public
HTTP tests pass login, membership-only warning display, CSRF rejection,
acknowledgment, logout rejection and cookie clearing. Ordinary warning routes
still pass their identity, escaping and CSRF checks. Complete browser coverage
and hosted CI remain pending.
Report membership, public badges and recovery/delivery remain unfinished. The complete posting flow still needs
browser qualification.


Non-member reply, image-reply and duplicate-image refusals now include the
source's fixed Pass information link. The Store selects it from the locked
CAPTCHA policy and validated membership result. Members, CAPTCHA-disabled
boards, staff replies and thread timers omit it. This flag changes presentation
only; it cannot select a cooldown or grant a bypass.

HTML escapes the ordinary message and renders a fixed link. Source JSON keeps
its single `error` field with the exact `S_RENZOKU_PASS` suffix. Quick Reply and
the drawing form recognize only that suffix and construct the link with DOM
methods; all other response text stays inert. The link retains the source's
external information URL. It does not connect the local entitlement store to an
external checkout. The separate checkout integration remains unfinished.

The Quick Reply bundle remains within its existing 50 KiB limit. Reusing the
unchanged upload-error string offsets the new renderer without removing guards.
Local checks pass the ordinary, member and CAPTCHA-disabled Store paths,
HTML/JSON responses and safe DOM rendering. The added browser cases still need
hosted qualification.

Quick Reply also halves its advisory timer when CAPTCHA is disabled or the
private endpoint confirms current Pass advice. Report exemptions and CAPTCHA
credits do not select that reduction. A presentation cookie only starts the
advice request; it cannot grant a bypass. Advice refreshes after submission.
The source browser halves milliseconds, so an odd five-second policy becomes
2.5 seconds in the advisory while the server still enforces its three-second
ceil-half rule. Unit tests preserve this distinction. The ordinary timer tests
remain unchanged; the CAPTCHA-disabled browser fixture projects 20/40 seconds
and checks the resulting 10/20-second advisory.


Pass preflight now reaches the public-board check before the ordinary Store
lookup. Its missing/private-board result is mapped back to `NotFound`, preserving
the existing public response. CI exposed the regression in private archive
qualification; the owned local test reproduced it before the correction and all
three private archive cases pass afterward. Invalid receipts and other database
errors retain their existing failure paths.


The historical-deletion operator's pinned guard now matches migration 0220,
which clears the public Pass year. The old pin safely refused the current schema
and was reproduced locally. An operator unit test compares both pins with the
latest migration bodies. The populated CLI qualification now erases an imported
year, preserves live and unrelated years, and rejects a guard whose year-clearing
statement was removed. Rollback, bounded batches and administrator restore pass.

The fresh-deletion HTTP fixture now uses distinct synthetic PNG pixels for its
live, deleted and archived cases. Reusing identical images made setup hit the
new source duplicate-image cooldown before it reached the erasure assertions.
The corrected test passes locally with the complete migration set; media
revocation, consumed-receipt rejection and surviving-content assertions remain
unchanged. The separate attachment-restore CI failure is still under investigation.

The attachment-restore fixture had the same setup defect. A local reproduction
confirmed the duplicate-image refusal before the correction. Each restore case
now has distinct synthetic pixels. The owned PostgreSQL 16 dump/file-restore
exercise passes for seven attachments and fourteen files, exact JSON and bytes,
four URL forms, deletion/archive visibility, one-use receipts, interrupted
cleanup and monotonic numbering. The source database remains unchanged. Local
qualification used an owned TCP cluster; the standard hosted wrapper still needs
CI qualification.

## Private report identity foundation

Migration 0225 keeps a report's validated Pass identity separate from public
content and the reported author's identity. A consumed receipt stages a handoff
bound to its transaction, actor, board and target. Membership insertion captures
it once; deferred cleanup removes unused handoffs. Retiring membership removes
the private association. Runtime roles cannot access these tables or stage a
handoff. The Pass catalog attests the capture and foreign-key triggers.

The reconstructed implementation passes the full local lifecycle suite, including
wrong targets/actors, replay, rollback, deferred cleanup, retirement, privilege
checks, trigger drift and populated restore with distinct author/reporter
identities. Public report receipt admission remains disconnected until the
remaining quota, ban, network and history integration is qualified.

Migrations 0226–0228 extend the private report boundary with source quotas,
explicit bans and network policy. Quotas combine address, automatic-session and
Pass matches once per report, preserving duplicate precedence and strict
15-second, hourly and daily cutoffs. Advisory GET has no consumed handoff.
Reporting keeps address and password ban checks even for members. Only a
matching consumed receipt or the stored CAPTCHA-off board policy skips network
rules; neither skips quotas or explicit bans.

The reconstructed SQL suite passes all lifecycle rollback and populated restore
checks. It covers cross-address duplicates, wrong actor/session bindings,
29/30-hourly and 79/80-daily limits, future timestamps, overlapping identities,
global/local/inactive bans, active warnings, and an owned blocked network range.
Ordinary Rust report regressions are being rerun. Public report receipt admission
still waits for retained clear-history integration and full end-to-end tests.

## Retained report-clear history

Migration 0229 records each manually cleared report and each new report that
inherits an existing clear. Capture runs after anonymous registration, preserving
the reporter's final private identity. The reported author's membership cannot
supply the reporter's identity. The history has no report foreign key, so later
report retirement does not remove it. Reporter purge and automatic clearing do
not invent manual-clear events.

The source keeps clear logs for five days. Native recording prunes older events
in bounded batches and limits retained rows. A full history inventory preserves
the valid report/clear operation and records an uncertainty horizon instead of
claiming complete history. Initial migration also starts a five-day warm-up.
Idempotent writes preserve the first timestamp and reject changed identities.
This foundation records identities and event counts; it does not claim an exact
source weight, enable history filtering, or issue automatic abuse bans.

Local SQL tests pass manual and inherited capture, duplicate clearing, retirement,
capacity uncertainty, expiry, privacy and populated restore. The 17 ordinary Rust
report challenge, group-clear, network and weight-evidence regressions pass with
this migration. Hosted qualification and public receipt integration remain pending.

Migration 0230 adds a private history decision for future report-weight integration.
It follows `modes/report.php:is_report_filtered`: a member's Pass identity replaces
the address branch, the retained password branch counts overlapping rows once,
clears use a strict two-day window, and three retired template-190 bans within
30 days meet the ban-history threshold. New-password requests omit the password
history branch. Positive retained matches remain usable during warm-up; a negative
result requires complete clear history and a verified, unchanged identity epoch.
Missing context or incomplete history returns unknown. Disabled thresholds return
false without consulting history.

The owned SQL matrix passes identity selection, overlap, new-password exclusion,
window edges, future timestamps, active/wrong-template exclusions, epoch changes,
runtime denial, migration rollback and populated restore. This helper does not
change effective weights or grant report admission by itself.
