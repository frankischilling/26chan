# Verified VIP posting

Issue #222 includes the source's separate VIP credential and `verified` badge.
This work is in progress. Private credential storage and receipt issuance are
implemented, but public posting does not consume those receipts yet. Parsing
the option alone grants no authority.

The preference boundary already discards VIP-looking options instead of saving
them in a cookie, and clears an existing options cookie after such a submission.
Native forms and Quick Reply also refuse to restore those values from old
cookies. This deliberately differs from the source's seven-day options cookie:
an identifier/password pair must not become a persistent display preference.
Case-varied and mistyped attempts are discarded too. Ordinary options and staff
badge selectors keep their existing preference behavior.

## Source behavior

`imgboard.php:4694–4760` accepts a `capcode_!identifier!password` option when
its caller supplies a name reference. The posting caller supplies that reference
only when its CAPTCHA-bypass predicate is true. CAPTCHA-disabled boards qualify
without a Pass; a presentation cookie alone does not establish this predicate.
Staff privileges and ordinary Pass membership are separate from VIP identity.

The caller removes every ASCII-case-insensitive occurrence of `sage` before
parsing the option, including occurrences inside the identifier or password.
The split uses at most three components, so further exclamation marks belong
to the password. PHP rejects the empty string and the exact string `0` in either
credential component. It does not trim spaces or reject every zero-like string.

The source counts failed logins at or after the one-hour boundary before
parsing the components. Three failures prevent lookup. A missing or inactive
account and a wrong password record a failure; missing components do not.
Successful verification replaces the displayed name with the configured VIP
name and updates last use. It emits `verified`, not a staff role.

The public label is `## Verified`, with `capcodeVerified` and `id_verified`.
It has no identity icon or highlighted-post state. The supplied schema describes
manually assigned identities; it contains no VIP account-enrollment interface.
`README2.txt` explicitly says its lengths, indexes and collations were inferred.
Those declarations do not establish the original database's identifier matching
rules.

The badge also changes particular posting predicates. The source's exact-file
blacklist, infrequent duplicate-image checks and Robot9000 check skip capcoded
posts. Ordinary content/network checks still run for a non-staff VIP, and the
retained banned-upload check still runs for every badge. Actor cooldowns,
thread quotas, ordinary bounds and staff privileges must not be inferred from
the badge alone. JPEG metadata stripping on `/p/` also depends on badge state;
its upload context must agree with the protected posting context.

## Qualification

The extractor pins `imgboard.php` by SHA-256 and retains only the VIP lookup,
VIP dispatch and badge fragments. The PHP harness substitutes owned rows and
records lookup, failed-attempt and last-use effects. It uses a synthetic password
hash and documentation address; it never loads the source application,
configuration, credentials or database.

The 288 cases cover CAPTCHA-bypass dispatch, active/inactive accounts, correct
and wrong passwords, failure thresholds, missing components, PHP truthiness,
extra exclamation marks, Unicode, spaces, prefix case and sage removal. Rust
compares lookup admission against those executed results. This does not qualify
database collation, concurrent failure accounting, receipt revocation, actual
posting, browser rendering or authority boundaries.

```sh
python3 scripts/extract-vip-capcode-reference.py /path/to/4chan-old --check
php -n scripts/test-vip-capcode-source.php
cargo test -p board-domain vip_capcode --locked
```

## Private issuer

Migration 0238 stores keyed identifier hashes and Argon2id password hashes behind
the isolated verifier. No runtime has raw table access. VIP identifiers use
exact UTF-8 spelling; the reconstructed schema's inferred collation is not
treated as evidence of the original matching behavior.

The issuer reserves an attempt before hashing and shares the existing
two-worker password budget with Pass login. Missing and inactive identities use
a dummy hash with the same bounded profile. Failed and interrupted attempts count
toward three attempts per peer in an hour; correct credentials release their
reservation. Global account, attempt and receipt inventories are finite.

CAPTCHA-disabled boards qualify directly. On CAPTCHA-enabled boards the issuer
checks an unconsumed current Pass receipt, including its peer, anonymous session,
form context, board and target. An ordinary CAPTCHA receipt does not qualify.
VIP authentication does not consume or replace the Pass receipt.

Completion checks the attempt's 30-second deadline and current credential
version before issuing a separate 30-second receipt. A name, password or active
state change advances that version. Receipts bind the account version to the
peer, anonymous session, board and target. The private response contains only
the opaque receipt and context hashes, with no password, identifier or name.
Posting-time consumption and its final revocation checks are still unfinished.

Local SQL qualification covers role denial, failure limits, changed credentials,
expired attempts, actual Pass receipt binding and revocation, catalog privilege
drift, migration rollback and populated restore. Private HTTP tests cover real
password verification and authenticated routing. Eight concurrent reservation
requests admit exactly three before refusing the rest.

`board-vip-admin` accepts a bounded JSON operation on stdin using
`MIGRATION_DATABASE_URL` and `VERIFICATION_KEY`. `create` requires `identifier`,
`name` and `password`; `active` defaults to true. `update` requires the identifier
and at least one of `name`, `password` or `active`. Its confirmation contains only
the internal account ID. Keep input outside the repository and protect it as
credential material. Identifiers are immutable and the verification key must
remain stable.

The provisioning command refuses credentials that cannot survive the source's
sage removal or fit its 100-byte options field. It rejects control characters
and does not put secrets in command-line arguments or output. The actual CLI tests cover create, password/name changes, deactivation,
runtime-role refusal and secret-free confirmation.

Public posting and Quick Reply submission, receipt consumption and the source's
badge-specific admission predicates remain unfinished. Credentials must not
enter post rows, logs, URLs or persistent browser preferences. Staff credential
and posting authority remain separate.

## Saved badge presentation

The renderer now distinguishes saved public badges from selectable staff
capcodes. `verified` is accepted by the public presentation type and remains
rejected by the staff-selection type. Desktop/mobile headers emit the source
label and title, without a staff icon, administrator highlight or Pass-year
decoration. Six pinned source stylesheets use `#007FFF` for the verified name,
trip and badge.

The shared updater/quote/search parser accepts the icon-free recipe and rejects
invented icons, substituted staff labels/classes, misplaced badges and
administrator highlighting. The filter and search bundles remain within their
existing release budgets. Local renderer and parser tests pass. Real-browser
highlight and inert-preview cases are written but cannot run in this workspace
because its pinned Chromium executable is absent; hosted qualification remains
required. This presentation work does not enable public VIP posting.
