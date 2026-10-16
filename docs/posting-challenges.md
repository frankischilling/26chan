# Posting and report challenges

Issue [#199](https://github.com/frankischilling/26chan/issues/199) is in progress.
Board policy, ticket storage, and normal post receipt enforcement are implemented
in the database. Public posting and Quick Reply still need to request and supply
those proofs; report enforcement and credit spending are unfinished. Email verification is covered in
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
posting issuer still needs to act on that advice.

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
cannot call it directly. Credit-backed receipts remain rejected until their
transactional spending path is implemented.

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

Staged-upload and report integration, credit spending, and browser flows still
need implementation or qualification. Existing posting fixtures must explicitly
account for the new gate. The branch has not passed full-workspace CI and must
not be treated as ready to merge.
