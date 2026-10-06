# Report-weight evidence

The domain modules `report_weight` and `report_threat` model a small part of the
supplied report pipeline. Migration 0102 captures a narrow set of admission-time facts. Staff clearance
and weighted queue ordering remain unchanged; old reports are not backfilled.

## Weight decisions

The reference is `modes/report.php:568–611` in supplied source revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Authenticated janitor-or-higher reporters
keep the category base. Other reporters receive weight 0.5 when they fail the
known-or-verified check, reach the threat threshold, or match enabled history
filtering, in that order. Otherwise they keep the base.

Missing facts remain unknown. The evaluator returns a weight only when every
possible completion agrees. A later proven fallback can establish 0.5 even when
an earlier condition is unknown; it cannot establish which ordered reason the
source would record. A base of exactly 0.5 also establishes the numeric result
without resolving those branches. Zero and negative finite bases are valid.

History evidence is conditional on a positive configured threshold. It is not an
unconditional result from the source helper. Disabled or negative thresholds do
not filter. The API does not serialize or accept request assertions as proof;
callers must establish source-equivalent facts separately.

## One-sided threat proofs

`lib/postfilter.php:2094–2097` adds 1.0 for its non-browser User-Agent expression.
Lines 2122–2124 add 0.8 when one of three incoming server fields is set:
`HTTP_PATH`, `HTTP_SAME_ORIGIN`, or `HTTP_REFERRER_POLICY`. All additions are
nonnegative; later multipliers increase the total before rounding to two decimal
places. Either signal establishes the report threshold of 0.4 without inventing
an exact score.

The bounded matcher accepts complete ASCII User-Agent bytes and the exact source
substring alternatives. Non-ASCII, truncated, or unavailable input does not
qualify a match. An independently qualified header-presence signal still works.
Empty header values count as present. URL paths, response policy headers, and
same-origin authorization checks are different facts.

No matching signal means unknown, never a proven low score. The remaining source
scorer includes browser detection, header order, cookies, timing and challenge
fields. An HTTP adapter still needs explicit rules for duplicate headers,
intermediaries and CGI-name collisions before these pure proofs can be used.

The hash-pinned extractor executes only the audited scorer function with synthetic
report inputs, without the original application bootstrap. The checked-in PHP
8.4.26 fixture has 79 cases: 51 positive proofs with full-source scores of at least
0.8, and 28 cases left unknown. Two unknown cases still have source scores above
0.4; they verify that absence of these signals is not evidence of a low score.
`scripts/extract-report-threat-reference.php` records the whole-file and extracted
function hashes. The Rust integration test consumes the actual scored output.

## Admission boundary

The known-or-verified predicate must use the resumed, locked session state before
the current report updates activity. An unlocked handler snapshot or a snapshot
after registration can change the answer. Public reporting currently carries no
source-equivalent authenticated staff session, so database role or cookie absence
cannot establish reporter staff status.

No historical backfill, weighted queue ordering, group-clear eligibility or
cross-board authorization is enabled. Unknown effective weights remain unknown
at those later boundaries.

## Private pre-report observation

Migration 0101 adds `post_secrets.report_known_or_verified` for the report
admission owner. It reenters the anonymous-session row lock and evaluates a local
copy of the resumed state with the source's 60-minute predicate. It neither
creates a session nor changes counters, pending activity, fingerprints, expiry or
identity. Callers must already follow board, report-gate, then session lock order.

Only the private report owner can call this helper. Public, staff, authentication
and migrator roles receive no direct execution grant. Migration 0102 calls it inside the two existing anonymous admission functions,
while their locks remain held and before registration advances report activity.

## Captured evidence

Migration 0102 writes one private row with the successful anonymous report. It
records the pre-report known-or-verified result and evaluator version. Staff,
threat, history and exact source-reason fields remain NULL. The table constraints
prevent this evaluator version from claiming those unavailable facts.

A categorical base of exactly 0.5 establishes effective weight 0.5 because every
possible branch agrees; its numeric proof is `BaseEqualsFallback`. All other
bases, including zero and negative values, remain unknown while staff authority
is unavailable. Free-text reports have no category base and no numeric proof.
The older IP-only report function does not create evidence.

The evidence row, report, membership and anonymous activity commit together.
Quota, category, registration and authorization failures roll them back. Existing
registration-time freshness and expiry checks remain in force. There is no
runtime setter or public evidence projection, and later catalog/session changes
do not reevaluate retained reports. Ordinary membership retirement keeps evidence
with report history; physical owner deletion of the report cascades its evidence.

The private table has at most one row per retained report. It shares that history's
retention policy rather than introducing a separate expiration that could erase
qualification while the report remains. Apply through migration 0102 before
starting matching services.
