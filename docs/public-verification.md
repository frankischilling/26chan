# Public CAPTCHA and email verification

Work in progress for #200. The email-verification routes, private issuer, mail
worker, and local mail fixture are implemented on the development branch. Full
browser, deployment, and final-head CI qualification remain
pending. Ordinary posting, Quick Reply, report CAPTCHA, tickets, and credits
remain #199 work. Neither issue is complete.

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
implementations. Tests use synthetic addresses and loopback fixtures; they never deliver
external email.

Migrations 0160 through 0162 add receipt-backed browser verification and current-browser
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
a separate service credential, bounded JSON bodies, and an eight-second exchange
limit. The private handler has a six-second budget. Enabling verification requires
a public handler budget of at least thirty seconds for its sequential calls. It
does not resolve hostnames, follow redirects, or retry writes. Its
protocol carries server-resolved session fingerprints rather than the browser's
raw deletion capability. Email-request challenges bind a form context so they
can load before the recipient is entered. The public process never receives either private verification key. A lost
verification reply can be reconciled only against the exact replacement browser
capability generated for that request. Writes are never retried automatically.
A disconnected client can still lose a successful response; a used link then
requires a new email request.

## Disposable development setup

After `scripts/dev-db.sh`, run `sudo bash scripts/dev-verification-db.sh` against
the owned development cluster. It refuses to overwrite existing credential files
or operate on a different cluster. The issuer and mail worker receive separate
`.local/verification-issuer.env` and `.local/verification-mail.env` files; matching
PowerShell files are generated too. Runtime keys must be provisioned separately; this script does not create them.

Only the explicit store, private issuer, and public verification round-trip test
commands source `.local/verification-tests.env`, which contains both synthetic
database roles. The regular public/staff workspace tests and browser commands do
not inherit those exports. Command-trace tests check
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

The focused public-handler round trip also passes against the real private HTTP
issuer and owned PostgreSQL: request, encrypted queue, link preview without
anonymous-cookie replacement, verification, capability rotation, and logout.
This is not browser or full-middleware qualification.

Still required for #200: real browser keyboard/mobile/lifecycle checks,
deployment validation, and all final-head CI checks. The source's sign-in stylesheet and
mini-leaf image are absent from the supplied archive. Twister fonts,
configuration, and private provider inputs are also missing. The bounded PNG
renderer is a functional replacement with its own glyphs; its images and the
sign-in page are not pixel-parity evidence. Provider/risk escalation remains an
explicit compatibility gap. No production test bypass is permitted.

## Public forms and browser state

`/signin` serves the email request or current-session logout form. A verification
link uses `/signin?action=verify&tkn=...`; GET checks the link without consuming
it. The link page does not set an anonymous cookie. A native "Continue
verification" POST returns to the same origin before resuming that cookie, which
keeps an external email link from replacing a SameSite=Strict browser session.
This extra step is a security-driven navigation difference. POST requires an independent HttpOnly form
secret, its matching nonce, and the public middleware's exact Origin check.
The form parser rejects duplicate decoded fields, malformed encodings, unknown
fields, and oversized requests. The issuer receives server-derived transport and
session information, never browser-supplied identity fields.

The CAPTCHA fetch goes through `/signin/challenge`. Only a successful sign-in
HTML response permits the verification script, that fetch endpoint, and inline
PNG image data in CSP. Other pages retain their previous image policy. Sign-in
pages use an origin-only referrer policy to keep link tokens out of asset
referrers while preserving native same-origin form submission.

The widget has keyboard-accessible native range and text controls, explicit
refresh, a bounded cooldown, and expiration three seconds before the issuer's
TTL. It clears proofs on page departure and persisted-page restoration. It does
not automatically refresh expired challenges or resubmit email mutations.
Successful verification rotates the anonymous browser cookie and sets the
non-authoritative `_ev1` hint for sixty seconds. Signout expires that hint.

## Local mail fixture and delivery

`board-verification-mailer --once` processes up to 100 messages using the
`board_mailer` role and the mail-encryption key. It claims one job at a time so
each delivery has a fresh lease. SMTP is restricted to an explicitly configured
numeric loopback endpoint, bounded replies, and a five-second exchange. Only a
final SMTP 250 response acknowledges delivery. An uncertain acknowledgement can
produce another delivery; its Message-ID stays the same. External authenticated
or TLS delivery belongs to the separately configured local relay.

For synthetic development mail, run:

```sh
python3 scripts/dev-verification-mail.py --output /tmp/owned-verification-mail --port 1025
```

The output directory must be new. The fixture binds only `127.0.0.1`, permits
four connections, and writes at most 100 private `.eml` files. Each message is
bounded to 8 KiB. During subsequent deliveries it removes its own messages older
than one hour. It does not serve files over HTTP. Stop it and remove its owned
output directory after testing; do not use real addresses or production tokens.

Configure the mailer with its separate database URL and key,
`VERIFICATION_SMTP_ADDR=127.0.0.1:1025`, and the local public origin. Run it with
`--once`, then open the synthetic message's verification link in the browser
being tested. No external email account is required.

Candidate service profiles are `deploy/verification.service` and
`deploy/verification-mailer.service`, with separate environment examples and a
mailer timer. Provision distinct OS users and root-owned 0600 environment files.
The verifier has the issuer database credential, identity key, mail key, and
service credential. The mailer gets only its delivery-role credential and mail
key. The public process gets only the loopback endpoint and service credential.
The profiles restrict network access to loopback and writable filesystem access;
they are not a substitute for qualification on the deployment host.

`test-public-verification-migration.sh` passes a populated pre-0158 upgrade and
an administrator dump/restore through 0163. It compares the retained anonymous
activity, linked identity, token counters, opaque queue payload, function owners,
ACLs, and definitions. Restriction and transfer readiness remain valid, and
runtime roles still lack private verification table authority. The queue payload
in this migration fixture is synthetic storage data, not a decryption test.

The dedicated verification browser fixture uses the production template and CSP
with synthetic challenge responses. Its Linux and Windows suites cover desktop
and mobile controls, native range-keyboard input, expiry, persisted restoration,
submission fields and Origin, and native preview/logout/completion views. Review
images are retained separately from later Playwright runs. These checks are
implemented but have not yet completed hosted qualification.

The mail-worker integration test runs the actual worker loop against owned
PostgreSQL and a loopback SMTP fixture. A final 250 acknowledgement removes the
encrypted outbox entry and records delivery. A 451 response leaves the message
queued with its retry delay and no delivery timestamp. The test never contacts
an external recipient.

Cooldown messages use the source's minute/hour wording. The policy reports the
remaining delay; the primary controller's base-delay branch displayed elapsed
minutes while still enforcing the five-minute limit. The rate limit is preserved,
and the backend's remaining-delay result is used consistently.

## Recipient and existing identity restrictions

Migration 0163 adds an exact-recipient blacklist containing keyed hashes. After
validating the CAPTCHA and cooldown, a blacklisted recipient or an active ban on
the requesting browser's identity receives the source's queued-success message
without creating a token or mail job. Board-specific identity bans also match
this request guard. Expired bans and warnings do not match it.

Token consumption rechecks the old browser, the target email identity, and the
recipient list before rotating the session. Operator INSERT, UPDATE, and DELETE operations take the same verification-policy
lock as enqueue and consumption. The recheck also covers a ban or blacklist
added after delivery. The restriction catalog is checked and held under the
existing shared capacity lock while the verification transaction completes.
Neither verifier nor public database credentials can read the ban rows or edit
the blacklist. Global range/ASN policy and Pass integration remain tied to their
separate source workflows; this guard does not claim those workflows complete.

An operator with the issuer's private configuration can run
`board-verification --recipient-hash`, supplying one address on standard input.
It emits only the 64-character keyed recipient hash and does not connect to a
database. Using the migration role, insert that hash into
`verification_private.blocked_recipients` with `decode(hash, 'hex')`; delete the
same row to remove the block. Never put the raw address or the verification key
in public SQL, logs, or command-line arguments. Delivery spelling is preserved,
so Gmail dot variants have distinct recipient-list entries even though account
identity normalization joins them.
