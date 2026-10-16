# Posting and report challenges

Issue [#199](https://github.com/frankischilling/26chan/issues/199) is in progress.
The board policy and ticket storage described here are implemented. Ordinary
posting, Quick Reply, report forms, and credit consumption
still need to use them. Email verification is covered in
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

Protected writes still need to consume a context-bound issuer receipt in the
same transaction as the post or report. The post-table insertion guard must
also cover staged uploads and direct public-role inserts. Credit grants must
roll back if any part of the protected write fails. Browser flows and these
write paths require their own tests before #199 can close.

Migration 0166 provides the private receipt-consumption helper for those writes.
It checks scope and expiry after acquiring its locks, consumes a matching proof
once, and rolls consumption back with a failed transaction. Runtime logins
cannot call it directly. It is not yet called by ordinary post or report writes;
credit-backed receipts remain rejected until their transactional spending path
is implemented.

The private credit profile uses resumed activity from before the current post,
matching the source's grant-before-activity ordering. It keeps the three grant
tiers and the separate sixty-second address-age requirement for new grants.
Existing-credit eligibility is computed separately. Verified browsers with the
same email identity share the private credit key; anonymous browsers keep
separate keys. This profile does not yet grant or spend stored credits.

The Store's typed receipt carrier installs transaction-local proof input and
clears any prior pooled-connection value when a writer supplies no proof. The
post-table guard still needs to consume that input; the carrier itself provides
no authorization.
