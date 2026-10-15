# Public CAPTCHA and email verification

Work in progress for issues #199 and #200. The public routes and forms are not
connected yet. These primitives do not establish completion of either issue.

## Source behavior

The reference is commit `545b7812d1849f7958d914950c91fdbbe38f6b22`, specifically
`captcha.php`, `lib/captcha.php`, `twister/twister_captcha.php`, `signin.php`,
`views/signin.tpl.php`, `tasks/signin_mailer.php`, and `lib/userpwd.php`.
The alternate `*-test.php` controllers are not the primary behavior contract.

Email preparation preserves the twelve allowed providers and Gmail identity
normalization. Delivery retains the normalized recipient's dots. The Rust parser
also bounds address length and accepts ASCII dot-atom local parts; quoted and
control-bearing addresses are rejected. The source fixture runs only the pinned
pure validation methods, with no application bootstrap, database, or mail call.

The CAPTCHA policy includes the 120-second lifetime, five-second refresh base,
unsolved-attempt escalation, posting waits, image style, and credit tiers.
A successful answer subtracts two unsolved attempts; credit qualification uses
the preceding count. The primary Twister answer normalization folds its
confusable glyphs before hashing.

Browser activity stays per browser, as it does in the signed UserPwd cookie.
Email identity and deletion ownership can be shared across verified browsers.
The planned verification transition preserves an unverified browser's resumed
activity, resets an already-verified browser's activity, and rotates the browser
capability. It must retain the seven-day idle verification reset.

## Authority and storage

Migration 0158 introduces private verification tables and an isolated issuer
role, `board_verifier`. The public database role cannot choose challenge answers,
mint receipts, or read private verification tables. Fresh challenge sessions
start with zero posting and reporting activity. Challenge reads do not update
activity or extend an existing session's lifetime.

Challenges bind the session, network identity, purpose, board, target, and action
context. Refresh replaces the previous challenge for that network identity.
Wrong and malformed answers consume the matching challenge. A context mismatch
cannot consume another browser's challenge. Successful verification creates a
short-lived receipt; only one concurrent answer can create it. Each protected
consumer must recheck current session authorization and consume the receipt in
its own transaction. Email enqueue already does this; posting and reporting
integration remain unfinished.

Migration 0159 adds encrypted email queuing and delivery leases. Enqueue applies
a five-minute network cooldown, one pending entry per exact recipient, two
requests per network per hour, and three per normalized email identity per day.
Tokens expire after 24 hours. Hashed single-use verification tokens replace the
source's reusable link as a security change.

The separate `board_mailer` role can claim up to 100 pending messages and
acknowledge its current lease. It cannot enqueue requests or answer challenges.
A lease lasts at most 60 seconds, failed delivery waits 60 seconds before retry,
and the encrypted outbox entry expires after one hour. Delivery decryption uses
a separate key and authenticates the request's token hash as context. Plaintext
recipients, bearer tokens, and keys have no automatic debug or serialization
implementations. Tests use synthetic addresses and never send email.

## Current evidence and remaining work

Focused tests cover source email preparation, secret separation, answer
normalization, authenticated mail payloads, fresh-session state, issuer role
drift, refresh delays, wrong/expired/replayed answers, concurrent receipt
creation, receipt-backed enqueue, exclusive delivery leases, and failed delivery
retry. Fresh PostgreSQL migrations and actual restricted-role calls pass in an
owned synthetic database. These tests use focused builds of the changed modules;
they do not replace the full workspace or hosted test suites.

Still required: browser verification and signout, durable ownership integration,
transactional post/report receipts and credits, ticket handling, the isolated
service and local mail fixture, challenge rendering, ordinary/Quick Reply/report
and sign-in forms, source-oracle coverage for remaining policies, upgrade/restore
checks, and full desktop/mobile CI qualification. Missing Twister fonts,
configuration, and private provider inputs need an explicit functional
replacement and documented fidelity limits. No production bypass is permitted.
