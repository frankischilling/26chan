# Authenticated staff network bypass

The source authenticates before it runs posting filters. Every authenticated
staff level, starting with janitor, receives both CAPTCHA and range bypass.
This includes unbadged staff posts and badged posts on `/test/`.

Earlier network tests followed the outer ordinary/test-board filter condition
without following the authentication call that precedes it. That produced
incorrect staff rejection expectations. The replacement checks the complete
caller path and keeps the public rejection tests.

## Source evidence

The pinned `lib/auth.php` hash is
`98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019`.
Its rank map orders janitor, moderator, manager and administrator. `has_level`
compares the requested rank with the authenticated rank. `valid_captcha_bypass`
sets both bypass flags for janitor-or-higher or local authentication, before
any Pass lookup.

In `imgboard.php`, both accepted CAPTCHA paths call that function before the
post-filter: directly for the normal path, or through `start_auth_captcha` for
the older CAPTCHA path. The later ordinary/test-board branch calls
`spam_filter_post_ip`, whose first checks return when the relevant bypass flag
is set. Its configured network and automatic-range queries are never reached
for these authenticated staff requests. The posting entry point separately
requires moderator-or-higher plus the developer flag for `/test/`. Public
requests and staff without that combination cannot use this posting target.

`extract-staff-network-bypass-reference.php` executes only this early-return
path from three hash-pinned functions. Sixteen isolated cases cover the four
staff ranks, CAPTCHA enabled/disabled and synthetic local authentication. No
Pass cookie, original application, deployment configuration or private list is
loaded. The fixture does not implement or qualify Pass or local-auth support
in the rewrite.

    php scripts/extract-staff-network-bypass-reference.php SOURCE crates/store/tests/fixtures/staff-network-bypass-reference.json --check

## Runtime boundary

The store checks the auth-pool role, exact session and CSRF hashes, a live
credential, session expiry, idle and recent-authentication clocks, account
revocation, actual staff rank and board access. Neither the selected badge nor
a caller-supplied moderator flag grants this exemption. Missing or invalid
authority fails before network policy can be skipped.

This early read grants only the network-filter exemption. It does not issue a
posting proof or retain auth locks across the content transaction. The existing
independent posting issuer rechecks authority after content locks, binds the
exact saved row and consumes its one-use proof before acceptance. A revocation
during the board-lock wait therefore still prevents the post from committing.
Holding the early auth locks would conflict with that issuer's transaction.

The bypass skips configured network policy and automatic-range lookup, including
unavailable hint/ASN providers and network-rule overflow. It does not skip
content rules, fixed name/trip checks, explicit posting restrictions, attachment
checks or final staff posting authority. It grants no anonymous deletion
membership, and does not broaden database privileges.

Staff accepted-post activity and cookies remain unfinished. Public and staff
use separate production cookie hosts. Passing staff through the public
anonymous ownership context would also change deletion proofs and other
session-dependent behavior; this change does not do that.

## Qualification

Database checks exercise all sixteen source cases, ordinary and badged test
posts, missing providers, network-policy overflow, invalid CSRF/session/role,
board denial, expiry, stale recent authentication and revocation. Test-board
controls cover public callers, janitors and missing developer flags. A real lock
wait checks revocation after the early read. Public callers retain automatic
refusal, attachment-expiry, rollback, scope, expiry and policy-concurrency tests.
The owned public control board has CAPTCHA disabled; it still enforces network
rules and does not gain the staff bypass.

The changed checkpoint passes 35 store/database tests, 32 staff posting
HTTP/database tests and both public posting/report HTTP tests. The local staff
suite excludes its unchanged Unix-socket case because this executor cannot bind
that socket; CI still runs it. All sixteen source cases reproduce, and strict
store/public/staff Clippy passes with all targets and features. Formatting and
diff checks pass.

The first converted public control reached the CAPTCHA guard after correctly
passing the range stage. Its owned fixture now explicitly disables CAPTCHA, as
the other isolated network fixtures do; runtime challenge enforcement is
unchanged. The complete test rerun passes.

## CI rejection-order correction

Full CI on `6c1bda48` caught an older quota-test expectation: it required the
private-board `P0002` refusal even when staff credentials were invalid. The new
authentication preflight rejects those credentials first. The assertion now
requires `AuthorizationChanged` for invalid credentials and `P0002` for a valid
identity using the ordinary writer on a private board. Both cases still require
an unchanged state snapshot. Runtime authorization and visibility checks were
not changed to satisfy the test.

The old assertion reproduces locally. The corrected run passes all ten quota
cases and 37 related database cases, including warning state, content rules,
staff attachments and refusal cookies. The local all-role fixture initially
lacked its monitor connection setting; supplying that owned test role allowed
the existing role-denial checks to run unchanged. Strict all-target/all-feature
store Clippy, formatting and diff checks pass. Fresh full CI remains required
for the corrected head; the earlier failed run does not qualify it.
