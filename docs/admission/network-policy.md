# Source network policy

The pure network-policy evaluator is under development for issue #212. It is
not yet connected to posting or reporting, and no operator rules are installed
by this change. Passing these tests does not mean network admission is active.

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
thresholds. An updated policy timestamp takes precedence over its creation time.

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

Posting must evaluate this stage after configured content rules and before the
active file blacklist and fixed name/trip stage. Reporting has its own source
options and lock order. Neither path is connected yet.

The server must supply canonical transport identity and a trusted ASN lookup.
Public forwarding or ASN headers cannot grant authority. Browser hints need a
bounded server-side derivation with operator-managed key material; source
embedded constants and private production lists must not be copied. Missing
providers, bypass eligibility and the mobile automatic-range stage still need
explicit handling and tests.

Policy updates must share the existing admission generation and transaction
locks. Add runtime role, rollback, ordering and concurrent policy-change tests
before treating the interface as complete. No hosted CI result is claimed for
this unintegrated foundation.
