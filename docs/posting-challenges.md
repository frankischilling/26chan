# Posting and report challenges

Issue [#199](https://github.com/frankischilling/26chan/issues/199) was closed by
[PR #292](https://github.com/frankischilling/26chan/pull/292).
Board policy, tickets, protected writes, and public challenge exchange are
implemented, along with normal posting, Quick Reply, and report controls.
Classic forms also carry server-rendered challenges for no-JavaScript submission.
All 23 checks on final head `289a5bbe` passed before merge, including the Linux
and Windows CAPTCHA browser suites and full Rust/database qualification. Email verification
is covered in [public verification](public-verification.md).

## Board policy

`fixtures/captcha-board-policy.json` records five independent switches for all
82 source board configurations, including boards outside the public board list.
The source revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.

- `/f/` enables CAPTCHA but selects the classic provider instead of Twister.
- `/hr/` sets the pre-upload CAPTCHA switch.
- `/pol/`, `/trash/`, and `/bant/` disable the bypass switch.
- The staff-only `/j/` configuration disables CAPTCHA.
- No captured board enables `NOSCRIPT_CAPTCHA_ONLY`.

The extractor reads global, category, and board overrides in that order. It
exports only these switches, board names, and file hashes. A provider switch
must not turn off verification. Migration 0165 keeps the switches separate and
checks the existing thread-transfer catalog before updating its seal for the
new board columns and narrow issuer grants.

The primary source does not emit `window.preupload_captcha` or handle
`mode=checkcaptcha` and `captcha_token`. Those fields occur only in the
experimental `js/extension-test-sync2.js`. The stored `/hr/` switch therefore
does not establish a working pre-upload protocol. Any future pre-upload gate
must verify a server-issued proof again when the final post consumes the upload;
network failure must not silently skip verification.

The classic provider's `NOSCRIPT_CAPTCHA_ONLY` branch suppresses its JavaScript
loader and retains the fallback form. Twister has no corresponding no-script
renderer in the primary source. Provider credentials and the Twister font are
deployment inputs missing from the reference checkout.

## Waiting tickets

The source's `captcha.php` uses a one-hour ticket bound to the board and peer.
A valid ticket retains its starting time across reloads and changes to the
required wait. At the expiry boundary, a new ticket starts a new wait. A
zero-second policy needs no ticket.

The replacement uses random `t1_` tokens and private, domain-separated hashes.
Only `board_verifier` can create or resume stored tickets. PostgreSQL supplies
the timestamps and checks board visibility, peer binding, expiry, and bounded
capacity. Ticket possession alone grants no posting or reporting permission.
The public database role cannot mint tickets or read the private ticket table.
If a lowered capacity still leaves too many rows after one cleanup batch, the
issuer receives an unavailable result while the expired-row cleanup commits.
Later attempts can continue that bounded cleanup. Valid ticket reuse does not
wait for the global issuance-capacity lock.
The ticket-advice API distinguishes missing tickets from valid waiting tickets
without creating either one. Missing or expired trusted risk input requires a
ticket challenge; a current score of 99 or 100 skips that escalation. This
keeps the source's score threshold without its user-agent exception. The
posting issuer applies that advice before creating a new ticket. Issuance
rechecks the precise risk expiry and score under the policy lock, so a
concurrent downgrade cannot leave the earlier advice usable. Only the private
completion path may substitute a consumed ticket-challenge receipt for that
trusted-score check.

The authenticated `/v1/posting/challenge` endpoint keeps ticket escalation
separate from the challenge used to submit a post or report. It uses an
eight-character local image when the source would require the external ticket
provider. Migration 0168 consumes that solved ticket challenge's receipt and
creates the waiting ticket in one transaction. The ticket still has to wait;
it cannot authorize a protected write.

Once the stored wait has elapsed, the issuer creates the ordinary action-bound
challenge. Reloads reuse the original ticket start. Classic-provider boards
request a non-slider replacement directly, without the Twister ticket protocol.
A disabled board returns an explicit disabled result. HTTP tests cover these
branches, ticket expiry, the trusted-score threshold, and ticket challenges
being rejected as posting proofs. The browser controller keeps ticket answers
separate from ordinary submission fields.

## Ticket validation

Domain tests cover wait boundaries, future timestamps, and separation from
challenge, email, and receipt tokens. Database tests cover ticket reuse,
cross-board and cross-peer rejection, expiry, capacity, and runtime privileges.
The populated verification upgrade/restore test also checks the ticket API and
preservation of the transfer catalog.

## Post transactions

Migration 0167 puts the receipt check on `content.posts` insertion, covering
direct public-role writes and the nested insertion used by staged attachments.
The Store's typed carrier installs transaction-local proof input and clears any
prior pooled-connection value when a writer supplies no proof. The database
checks the private issuer receipt rather than trusting that input.

Migration 0166 provides the private receipt-consumption helper for those writes.
It checks scope and expiry after acquiring its locks, consumes a matching proof
once, and rolls consumption back with a failed transaction. Runtime logins
cannot call it directly. Its normal-proof interface rejects credit claims;
migration 0169 adds a separate private receipt-data interface for the posting
guard and its transactional credit path.

The private credit profile uses resumed activity from before the current post,
matching the source's grant-before-activity ordering. It keeps the three grant
tiers and the separate sixty-second address-age requirement for new grants.
Existing-credit eligibility is computed separately. Verified browsers with the
same email identity share the private credit key; anonymous browsers keep
separate keys.

A private post/session/transaction handoff carries grant eligibility from the
insertion check to anonymous activity registration. Registration applies any
eligible credit grant before advancing activity. A deferred constraint prevents
an unfinished handoff from committing. Post failures roll back the receipt,
handoff, and credit grant together. Staff and migration writes retain their
existing separate authority. Readiness rejects a missing or disabled guard.

Focused database tests cover direct missing-proof rejection, a valid normal
post, replay rejection, incomplete handoffs, readiness drift, and the difference
between four and five posts before the new post. Staged-attachment tests force
a failure after the nested post insertion, verify that the receipt and post
roll back, then commit a retry with the same upload and proof. The email-verification
content-filter fixture disables CAPTCHA only on its owned test board so that it
continues to isolate that separate admission rule.

The authenticated `/v1/check` endpoint exchanges a solved thread, reply, or
report challenge for a thirty-second receipt. HTTP tests verify purpose and
form binding, stored receipt scope, wrong-answer consumption, and replay
rejection. It does not consume the receipt or complete a protected write.

## Credit-backed replies

Migration 0169 issues short-lived credit claims bound to the browser's current
private credit identity. Issuing a claim does not decrement the balance. The
reply transaction checks board policy, current risk, resumed activity, identity,
and credit expiry again, then conditionally spends one credit before activity
registration. Credit-backed replies cannot replenish credits.

Tests force a registration failure after spending and verify that the balance
and receipt are restored. Two concurrent claims for the same last credit admit
only one reply. Other cases revoke board bypass or trusted risk after issuance,
expire the balance, or substitute a different private identity; all reject the
write without consuming its claim.

Migration 0170 separates credit advice from claiming a credit. Repeated
challenge refreshes return the available count and bounded lifetime without
allocating receipts or changing the balance. The authenticated `/v1/credit`
endpoint supplies a reply/report claim for immediate submission; it refuses
thread claims and rechecks current eligibility. The protected write still owns
consumption. HTTP tests cover refreshes without allocation, multiple claims
without spending, and revoked board bypass. Readiness checks the lifetime-aware
SQL result shape; upgrade tests exercise actual verifier startup against both
the old and new contracts.

## Report transactions

Migration 0171 checks report proofs inside both free-text and categorical
admission. The existing target and session checks precede CAPTCHA; quotas,
restrictions, category checks, report insertion, and activity registration then
run in the same transaction. A later failure restores the receipt and any
spent credit. Report submissions never grant new credits.

Actual public-role tests cover normal and credit proofs in both modes, missing
proof, stale category revision, late registration failure, success, and replay.
They also verify that the separate staff-only report path remains usable and
that an explicitly CAPTCHA-disabled test board accepts a report without proof.
Public posting and reporting handlers now obtain private issuer proofs and
pass them through the protected Store APIs. Browser form integration remains
in progress.

The source's optional no-CAPTCHA report exemption uses retained history. Existing
report memberships can disappear during retirement, so they cannot establish
complete hourly report or recent-clear history. Source clear history comes from
target/group clearing and new reports against an already-cleared group; the
reporter-wide clear logger is commented out.

Migration 0172 retains each private report actor's latest three successful
report times and latest target-clear time independently of report foreign keys.
The membership trigger captures new reports and inherited group clears; target
clearing records each affected actor while holding the shared report gate.
Deleting a report or target does not erase these windows. Tests cover ordinary,
idempotent, and inherited clears, retirement, and failed-report rollback.

History capacity is bounded. Cleanup removes at most 64 expired actor rows per
call. Existing actors can still be updated at full capacity. If new history
cannot be recorded, the store marks history incomplete through 72 hours after
the latest omitted event, and at least 72 hours from the current time, for the
eligibility check to honor. It does not prevent CAPTCHA-backed reporting or
moderator clearing. Bulk membership inserts above the 10,000-event capture
bound also mark history incomplete rather than rejecting the existing bulk
operation. Initial
history also needs a 72-hour warm-up because retired events cannot be recovered.
Migration 0174 checks this state during report admission, before consuming a
proof, and keeps its lock until the transaction commits.

Migration 0173 tracks a private fingerprint of the report and restriction
identity key. A known key change preserves the existing incomplete-history
watermark, every retained report/clear expiry, and the latest retained
restriction event plus thirty days. It does not restart an elapsed warm-up on
a quiet site. Missing or malformed epoch metadata extends the conservative
window by thirty days. Normal report transactions retain the restriction gate
while observing this state; administrative imports only invalidate history.
Failed reports roll back epoch changes with the rest of admission.

This guards the recent-event history windows. It does not migrate active bans
when an operator replaces the identity key. Keep that key stable. Migration
0174 permanently marks optional-exemption continuity uncertain after a key
change or missing epoch metadata. First adoption also marks it uncertain if
active bans or warnings already exist. Retired restriction events still age
out normally. Later requests cannot clear this flag; restoring it would need a
separate operator-reviewed migration. This fallback requires ordinary CAPTCHA
and does not restore matching of active bans created under an older key.

The source's attachment predicate is positive retained `fsize`, even after a
file-only deletion. Its optional exemption is independent of the credit-bypass
board switch. Either `4chan_auser` or `pass_enabled` cookie suppresses it;
those cookie names do not establish authentication in this rewrite. The stored
anonymous session is the replacement for source UserPwd state, which came from
its separate `4chan_pass` cookie.

The optional exemption requires a valid stored browser session, seven days at
the same exact address, a source-compatible IPv4 peer, positive retained
attachment bytes, fewer than three reports in the last hour, no target-clear
event in the last 72 hours, and no warning or ban event in the last thirty days.
An idle reset or address/network change removes eligibility; an environment
change alone does not. The Store carries the route's legacy-cookie opt-out
separately from the server-derived transport identity.

Tests exercise both free-text and categorical admission, including a deleted
file on a board with credit bypass disabled. They reject young or changed
addresses, idle sessions, IPv6, text-only targets, recent reports and clears,
retired warnings, incomplete history, uncertain key continuity, and exhausted
history capacity. A forced late failure restores report history. Concurrent
attempts to use the third exempt report admit only one. Separate SQL tests cover
the strict thirty-day edge and persistent uncertainty after key changes.

The report advice API applies the same predicate without recording a report or
advancing browser activity. Advice does not reserve eligibility: final admission
checks again, so a third report or intervening clear can require CAPTCHA after
the form was opened. Tests verify repeated advice leaves activity and report
history unchanged and that advice becomes false after the third report.

The public `/captcha` POST endpoint constructs scope from the server-resolved
browser and transport peer. Its separate HttpOnly `board-captcha` cookie binds
form nonces without disturbing sign-in forms. The response contains challenge
images, waiting-ticket state, or report advice; private receipt, peer, and
context hashes stay inside the services. The endpoint rejects browser-selected
internal purposes, duplicate fields, invalid tokens, and stale form nonces.
A real-role HTTP test covers classic issuance, ticket escalation and reuse,
report-cookie opt-out, and a disabled board with the issuer offline.

Normal and legacy posting/report forms accept bounded CAPTCHA nonce, answer,
challenge, and credit fields. The public handler checks the form cookie before
requesting a private receipt, then sends the receipt to the existing protected
write. Reply/report credits are claimed immediately before submission and still
spent only by the write transaction. Thread submissions cannot claim a credit.
A changed exemption is rechecked before and during final admission. Missing
issuer service, wrong scope, replay, and stale forms fail closed. Receipt
rejection returns a fixed CAPTCHA message without private database diagnostics.

## Browser controls and qualification

The normal form, Quick Reply, and report form share a bounded controller. It
loads challenges on demand, exposes the image alignment slider, honors refresh
cooldowns, and clears answers three seconds before expiry. Ticket answers start
the waiting period; its timer may request an ordinary challenge but never submits
a post. Waiting tickets are saved per board. Storage failure leaves in-memory
reuse available.

Credits display “Verification not required.” and populate a hidden claim field.
They are not a checkbox or an entitlement chosen by the browser. Report exemption
hides the controls while retaining its form nonce. Both decisions are checked
again by the server on submission.

Classic forms include a real challenge image, token, and form nonce in their
HTML. They can submit without JavaScript. `NOSCRIPT_CAPTCHA_ONLY` suppresses the
controller and Quick Reply; the native form remains available. Twister retains
its source requirement for JavaScript. The replacement uses local raster images;
it does not claim the missing provider's visual output or credentials.

Quick Reply creates fresh controls for its current target. Retargeting, closing,
failed submission, and persisted-page restoration clear stale proofs. Concurrent
main-form and Quick Reply initialization is serialized so their first responses
do not replace each other's anonymous/form cookies. CAPTCHA requests have a
bounded timeout, and a failed request requires manual retry after a delay.

Successful marked HTML responses alone receive inline PNG and `/captcha` CSP
authority. Ordinary board pages can import the shared module for Quick Reply
without gaining that network or image authority. Unit tests cover marked,
unmarked, error, and JSON responses.

The local no-TLS public build and controller state tests pass. The Unix-socket
transport test cannot run in this environment because socket creation is
denied; it remains enabled for CI. Real-role tests cover public challenge
exchange and classic server-rendered submission. These checks do not replace
the locked production build or browser qualification.

The hosted browser suite now includes posting, reporting, classic/no-script,
credits, tickets, expiry, failure, and Quick Reply cases. These are synthetic UI
fixtures using the production template, modules, and CSP; real database proof
consumption is tested separately. All 21 verification/CAPTCHA browser cases
passed on Linux and Windows in [run 38881524160](https://github.com/frankischilling/26chan/actions/runs/38881524160),
and the desktop/mobile captures were reviewed. Full-workspace qualification of
the final head is still required before merge. Unrelated tests opt out only on
their owned fixture boards; production defaults and dedicated challenge tests
retain the gate.

Source-specific browser cases keep their original board names when those names
are part of the behavior being tested. They use an explicit per-test CAPTCHA
policy lease through the existing disposable-database fixture authority. The
helper requires a private run manifest, loopback migration database, exact live
run identity, and synthetic database witnesses. Only seven named fixture boards
are admitted. It records the full original board row before changing the CAPTCHA
flag and refuses to restore over any unrelated policy change. Cleanup restores
the original flag, including when the test fails; run teardown also recovers
remaining owned leases. Dedicated CAPTCHA tests do not use this helper.

PostgreSQL tests check exact restoration, duplicate/foreign lease rejection,
changed-policy refusal, idempotent cleanup, and run-teardown restoration. The
JavaScript helper rejects nested scopes and attempts cleanup for every board
whose setup was attempted. The source-specific media cases retain their own
explicit, guarded `/i/` and `/gif/` leases.

Issuer readiness checks the login identity before calling issuer-only database
functions. A mailer or public connection is rejected as an unsafe role without
relying on PostgreSQL's evaluation order for `AND` expressions. The delivery
integration test covers this separation alongside its receipt and lease checks.

The shared demo seed remains loadable before migration 0165. Its CAPTCHA
opt-outs run only when that column exists and match each synthetic board's slug,
title, and description. Historical upgrade tests keep their earlier schemas.
The still-image HTTP fixture retains the real `/aco/` name required by the
pinned decoder policy and restores its original CAPTCHA flag after the case.

The staff browser fixture also disables CAPTCHA on its newly created synthetic
board, so spoiler and moderation cases can test their own public write paths.
The file-deletion upgrade test extends its saved board baseline with the five
CAPTCHA policy fields from the pinned fixture. It still compares every prior
field and qualifies rollback and administrator dump/restore. The anonymous-session
upgrade test keeps its historical schema unchanged, then opts out only its
synthetic board before exercising current public ownership and report writes;
it also verifies that `/b/` retains CAPTCHA enforcement.

The complete archive reader now requests only the subject and deletion flag
from each root-post lookup. Its row and byte limits, visibility rules, ordering,
and five-second public database timeout are unchanged. The 100,000-entry boundary
test checks every returned ID and subject. A local PostgreSQL 16 comparison found
the narrower lookup faster in both alternating runs; hosted CI still determines
whether the full suite passes on the final commit.
