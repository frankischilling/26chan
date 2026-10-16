# Source network policy

Network admission is under development for issue #212. IPv4 range rules now
run in the posting transaction, including ordinary staff posts. Migration 0178
adds private operator-owned rules and a scoped reader; it installs no rules.
The offline ASN provider is wired into public and staff posting. Browser and
request-signature providers, reports and automatic range bans remain unfinished.

## Covered decisions

`content_admission::network` evaluates bounded operator-owned IPv4 ranges and
ASN selectors. Candidates must be active and unexpired. Source ordering is
leniency ascending; ties retain the caller's explicit order because the source
SQL does not define a tie-breaker.

Rules retain global, individual-board and work-safe scope, report-only, OP-only
and image-only restrictions, and literal browser/request-signature matching.
The three leniency modes use the existing request-adjusted anonymous activity
state. Verified status bypasses the post-count condition where the source does;
known-only exemptions retain the special OP/image post-count and network-age
thresholds. An updated policy timestamp takes precedence over its creation
time. Ordinary operator edits refresh that timestamp; explicit changed timestamps remain
available for historical imports. Activity ages use the locked session snapshot
clock, while rule expiry uses the request-start clock.

Source PHP truthiness matters for the exact string `0`: a board-scope field of
`0` is global, but `0,g` and `0,` remain explicit lists. The same falsey-string
rule applies to signature filters and supplied signature values. Empty list
entries are preserved rather than collapsing those distinct inputs.

The module caps rule count, scope size and signature bytes. It rejects invalid
rule IDs, duplicate IDs, reversed ranges, empty selectors, zero ASNs and invalid
board scopes. It does not execute patterns or arbitrary callbacks.

## Source evidence and fixture

The extractor selects only `isIPRangeBanned` from the hash-pinned
`lib/postfilter.php` and seven activity methods from `lib/userpwd.php`. It does
not load the application, constructors, cookie codecs, configuration, request
secrets or operator lists. Synthetic database rows stand in for candidate
selection; this is not a database-query conformance test.

The fixture contains 1,392 executed cases covering scope, expiration boundaries,
range-or-ASN matching, literal signatures, leniency ordering, verified status,
post/report counts, pending activity and updated-versus-created timestamps.
An expanded corpus caught the initial implementation's `0` truthiness mismatch
before integration. The Rust evaluator now preserves the source distinction.

Regenerate against the supplied source checkout:

    php scripts/extract-network-admission-reference.php SOURCE fixtures/network-admission-reference.json

Check an existing fixture by appending `--check`. The extractor verifies both
source-file hashes before selecting any body. The normal domain test suite
compares the Rust decisions against the checked-in fixture.

## Remaining integration

Posting evaluates this stage after configured content decisions and before
fixed name/trip checks. A later range rejection rolls back earlier log/autosage
effects. Content rejection and quiet success still short-circuit first. The
active file-blacklist stage remains to be inserted before fixed identity checks.
Reporting has separate source options and lock order and is not connected yet.

The server must supply canonical transport identity and a trusted ASN lookup.
Set `ASN_DATABASE` for the public service and `STAFF_ASN_DATABASE` for staff.
Each is an absolute path to a bounded, verified [ASN database](asn-fixture.md),
loaded at startup. Invalid configured databases prevent startup. The store
resolves the canonical trusted peer before opening the posting transaction;
callers supply the database, not an asserted ASN or a separately resolved value.
Public forwarding or ASN headers cannot grant authority. A successful lookup
with no match is distinct from an unavailable provider. Provider-dependent rules
fail closed only when the corresponding ordered stage is applicable; expired,
out-of-scope or report-only rules do not require posting providers. Browser
hints need a bounded server-side derivation with operator-managed key material; source
embedded constants and private production lists must not be copied. Signature
providers, bypass eligibility and the mobile automatic-range stage still need
implementation and tests.

The source posting caller uses PHP `ip2long` truthiness to skip IPv6 and
`0.0.0.0`. This implementation deliberately closes that bypass: configured ASN
rules also apply to canonical IPv6 peers, and IPv4 ranges include zero when the
operator selects it. IPv4-mapped IPv6 uses the same lookup as IPv4. This is a
security difference, not a claim that the source applies IPv6 range rules.

Policy updates share the existing admission generation and transaction locks.
Database cases cover content/network precedence, rollback, source OP/image
scope, expiry, unavailable providers, runtime-role denial and updates waiting on
the shared generation. Ordinary moderator, manager, admin and janitor posts
cannot bypass a range rule; badged staff retain their existing normal-board
exemption. Full provider and report qualification remain necessary.

Current error text distinguishes files, threads and all posts, temporary versus
permanent blocks, and known/verified leniency. Source error links and the Pass
bypass footer remain unfinished; no unsupported bypass is advertised. No hosted
CI result is claimed for this later posting integration.

## Local qualification of posting integration

Standalone manifests point at the production source files, with SQLx TLS
disabled only for owned PostgreSQL 16 TCP fixtures. The IPv4 posting checkpoint
passes the complete 297-test domain suite. Domain, store and public Clippy pass
with warnings denied.
Fourteen content-admission, one randomizer and fifteen staff-attachment database
tests pass. The new cases check fixed identity precedence, content-rule
short-circuiting, network rejection rollback, shared-generation contention,
ordinary staff scope and unavailable-provider handling.

The actual public HTTP test passes its existing content/generated matrices and
eight network rejection cases across both posting routes, form encodings and
response modes. Irrelevant forwarded-address and ASN headers cannot replace the
fixture's trusted socket peer. Removing the owned rule admits the next request.
The populated 0178 upgrade preserves historical content and audit rows, verifies
ownership/grants, and checks scoped reads and the 129-row overflow sentinel
through a real public-role connection.

Two fixture mistakes were corrected during qualification: HTML rule errors use
422 while JSON rule errors use 200, and the migration role deliberately cannot
assume the public runtime role. The upgrade harness now connects as the public
role instead of granting extra membership. Neither correction changed runtime
limits, grants or response behavior.

Full hosted qualification of this later integration is pending. Earlier
checkpoint runs do not establish the result for a changed tree.

## ASN integration qualification

The expanded actual HTTP test passes matching and mapped IPv4, canonical IPv6,
a different ASN, a successful no-record result, an absent provider and continued
evaluation of a later range rule. All 30 database cases pass with ASN coverage
for ordinary staff and the badged `/test/` exception. The staff loader unit also
passes valid, missing, relative, directory and malformed database cases.
Store, public and staff Clippy pass with all targets/features and warnings
denied. The configuration role-isolation tests pass, and 12 actual-binary
startup cases reject invalid databases and cross-role settings before binding
or database access. Those local binaries use the same no-TLS fixture manifests
and the local ICU library path; full production builds remain a hosted check.

The first HTTP repeat expected 500 for an unavailable provider; the existing
public error mapping uses 503. The assertion was corrected and the full test
passed without changing runtime response behavior. Hosted qualification remains
pending; results from earlier branch heads do not qualify these changes.
