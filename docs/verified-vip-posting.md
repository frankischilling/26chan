# Verified VIP posting

Issue #222 includes the source's separate VIP credential and `verified` badge.
This work is in progress. The option parser and source qualification do not
enable verified posts, create credentials or grant authority.

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

Private credential storage, bounded hashing and attempts, one-use posting
receipts, operator provisioning, public/native/Quick Reply projections and
transactional revocation checks remain to be implemented and qualified.
Credentials must not enter post rows, logs, URLs or persistent browser
preferences. The existing staff credential and posting boundaries must remain
independent.
