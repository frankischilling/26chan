# Report-weight evidence

The domain modules `report_weight` and `report_threat` model a small part of the
supplied report pipeline. They are not connected to admission or staff clearance.
Existing reports have no inferred effective weight.

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
cross-board authorization is enabled by these modules. Unknown effective weights
must remain unknown at those later boundaries.

## Private pre-report observation

Migration 0101 adds `post_secrets.report_known_or_verified` for the report
admission owner. It reenters the anonymous-session row lock and evaluates a local
copy of the resumed state with the source's 60-minute predicate. It neither
creates a session nor changes counters, pending activity, fingerprints, expiry or
identity. Callers must already follow board, report-gate, then session lock order.

Only the private report owner can call this helper. Public, staff, authentication
and migrator roles receive no direct execution grant. It remains unwired until
admission can bind its result to the exact report and establish the other required
facts. Adding this helper does not change report acceptance or staff clearance.
