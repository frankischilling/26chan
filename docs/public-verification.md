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

A second pinned fixture checks twenty Twister normalization cases, including
confusable glyphs, punctuation, Unicode stripping, and explicit Rust input bounds.

The CAPTCHA policy includes the 120-second lifetime, five-second refresh base,
unsolved-attempt escalation, posting waits, image style, and credit tiers.
A successful answer subtracts two unsolved attempts; credit qualification uses
the preceding count. The primary Twister answer normalization folds its
confusable glyphs before hashing.

Browser activity stays per browser, as it does in the signed UserPwd cookie.
Email identity and deletion ownership can be shared across verified browsers.
The verification transition preserves an unverified browser's resumed activity,
resets an already-verified browser's activity, and rotates the browser capability.
The seven-day idle verification reset remains in effect.

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

Migrations 0160–0162 add receipt-backed browser verification and current-browser
signout. Concurrent verification can rotate a browser only once. Signout leaves
ordinary anonymous sessions alone. For a linked browser, it revokes the capability
even after an idle reset; this is a security distinction from the source, which
only clears a cookie whose resumed verification level is positive.

Newly registered verified posts retain private deletion ownership independently
of the originating browser's lifetime. Linked browsers share automatic admission
equality while keeping separate activity counters and anonymous UUIDs. Earlier
anonymous posts are not assigned to the email identity. Staff text transfer
copies and verifies durable ownership with the other private evidence; whole-post
erasure removes the proof. The existing restriction and transfer catalogs are
checked before their reviewed dependencies change and resealed afterward.

The source's CAPTCHA failure budget is six normally and three for suspicious
verification. The queued request retains its limit; a suspicious verification
challenge can tighten it. These counters are separate from the five failed
country-check allowance and the single-use token state.

The internal verification client uses a fixed numeric loopback endpoint,
a separate service credential, bounded JSON bodies, and a twelve-second exchange
limit. It does not resolve hostnames, follow redirects, or retry writes. Its
protocol carries server-resolved session fingerprints rather than the browser's
raw deletion capability. Email-request challenges bind a form context so they
can load before the recipient is entered. The issuer process and public routes
still need implementation.

## Disposable development setup

After `scripts/dev-db.sh`, run `sudo bash scripts/dev-verification-db.sh` against
the owned development cluster. It refuses to overwrite existing credential files
or operate on a different cluster. The issuer and mail worker receive separate
`.local/verification-issuer.env` and `.local/verification-mail.env` files; matching
PowerShell files are generated too. Runtime keys and service startup still need
integration.

Only the store-test subprocess sources `.local/verification-tests.env`, which
contains both synthetic database roles. The regular public/staff workspace tests
and browser commands do not inherit those exports. Command-trace tests check
that separation and retain every prior verification prerequisite.

## Current evidence and remaining work

Focused tests cover source email preparation, secret separation, answer
normalization, authenticated mail payloads, fresh-session state, issuer role
drift, refresh delays, wrong/expired/replayed answers, concurrent receipt
creation, receipt-backed enqueue, exclusive delivery leases, failed delivery
retry, browser rotation, independent activity, durable ownership, and transfer
and erasure of that ownership. Fresh PostgreSQL migrations and actual restricted-role calls pass in an
owned synthetic database. These tests use focused builds of the changed modules;
they do not replace the full workspace or hosted test suites. A separate local
harness also compiles a byte-identical copy of the complete store source against
the actual domain crate and runs the existing anonymous-session and transfer
suites. That harness uses local no-TLS dependencies, so it does not qualify the
normal locked application build.

Still required: public route/form integration for verification and signout,
transactional post/report receipts and credits, ticket handling, the isolated
service and local mail fixture, challenge rendering, ordinary/Quick Reply/report
and sign-in forms, source-oracle coverage for remaining policies, upgrade/restore
checks, and full desktop/mobile CI qualification. Missing Twister fonts,
configuration, and private provider inputs need an explicit functional
replacement and documented fidelity limits. No production bypass is permitted.
