# Posting and report challenges

Issue [#199](https://github.com/frankischilling/26chan/issues/199) is in progress.
The board policy and ticket storage described here are implemented. Ordinary
posting, Quick Reply, report forms, pre-upload controls, and credit consumption
still need to use them. Email verification is covered in
[public verification](public-verification.md).

## Board policy

`fixtures/captcha-board-policy.json` records five independent switches for all
82 source board configurations, including boards outside the public board list.
The source revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.

- `/f/` enables CAPTCHA but selects the classic provider instead of Twister.
- `/hr/` enables pre-upload CAPTCHA.
- `/pol/`, `/trash/`, and `/bant/` disable the bypass switch.
- The staff-only `/j/` configuration disables CAPTCHA.
- No captured board enables `NOSCRIPT_CAPTCHA_ONLY`.

The extractor reads global, category, and board overrides in that order. It
exports only these switches, board names, and file hashes. A provider switch
must not turn off verification. Migration 0165 keeps the switches separate and
checks the existing thread-transfer catalog before updating its seal for the
new board columns and narrow issuer grants.

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
