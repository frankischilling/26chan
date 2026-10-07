# Report-weight evidence

The domain modules `report_weight` and `report_threat` model a small part of the
supplied report pipeline. Migration 0102 captures a narrow set of admission-time
facts. Migration 0108 uses those facts for bounded ordinary report-group clearing.
Weighted queue ordering remains unchanged; old reports are not backfilled.

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

## Source numeric precision boundary

The branch decision and the exact persisted weight are separate claims. The
supplied `README2.txt:1-3` describes a schema reconstructed from PHP analysis with
inferred details. Its `FLOAT` declarations for report and category weights at
lines 233 and 272 are not authoritative deployed DDL.

`modes/report.php:634-642` inserts the selected weight through `%F`.
`lib/db.php:209-221` formats the query with `vsprintf`, so this path includes a
numeric formatting boundary before database storage. The optional adapter in
`lib/db_pdo.php:209-231` instead prepares the query and binds arguments as integer
or string parameters. The supplied adapters alone do not establish which path
and runtime were deployed or the resulting storage precision. The queue sums
stored report weights, as shown in `reports/ReportQueue.php:813-820` and
`846-851`; it does not sum the evaluator's unpersisted branch results.

A future source-equivalent staff proof could establish the category-base branch.
It would not, by itself, prove the exact persisted value of an arbitrary base.
That requires authoritative DDL and the deployed adapter/runtime contract,
including formatting and storage conversion. This boundary does not change the
current exact-0.5 proof or enable arbitrary-base evidence.

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

Migration 0108 enables a bounded board-scoped group clear for a recently
authenticated janitor or higher with access to that board. Every current member
must have captured, non-NULL effective-weight evidence, and the sum must be
finite and nonzero. Groups over 10,000 members are rejected without clearing a
subset. The current proof supports only the opt-in categorical path with base
0.5; default free-text reports and historical unknown weights cannot authorize
a fresh clear.

Later admissions inherit a surviving group's clear state even when their own
weights are unknown. Inheritance records an existing clear; it does not prove
their weights or authorize a new clear. No category catalog activation,
historical backfill, weighted queue ordering or weighted cross-board unlock is
enabled. See [ordinary report-group clearing](report-group-clear.md) for the
full contract and remaining limits.

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
