# Posting and report challenges

Issue [#199](https://github.com/frankischilling/26chan/issues/199) is in progress.
Board policy, ticket storage, and normal post receipt enforcement are implemented
in the database. Public posting and Quick Reply still need to request and supply
those proofs; the report exemption and browser credit controls are unfinished. Email verification is covered in
[public verification](public-verification.md).

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
being rejected as posting proofs. Browser integration remains unfinished.

## Validation and remaining work

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
The public handlers still need to supply proofs through the new Store APIs.

The source's optional no-CAPTCHA report exemption remains unfinished. Existing
report memberships can disappear during retirement, so they cannot establish
complete hourly report or recent-clear history. Source clear history comes from
target/group clearing and new reports against an already-cleared group; the
reporter-wide clear logger is commented out. The replacement needs retained,
bounded history and tests for those exact events before enabling the exemption.

Report exemption and browser credit controls still need implementation or
qualification. Existing posting fixtures must explicitly
account for the new gate. The branch has not passed full-workspace CI and must
not be treated as ready to merge.
