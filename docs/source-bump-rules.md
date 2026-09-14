# Source bump rules

[Issue #114](https://github.com/frankischilling/26chan/issues/114) covers
surviving reply counts, the post-insert cutoff and sticky-thread suppression.
The supplied `imgboard.php:6627-6653` counts nonarchived replies after inserting
the incoming post. For ordinary threads, sage or a count at least `MAX_RES`
prevents a bump. Sticky threads skip bumping independently of that count.

The store counts undeleted replies under the same board transaction lock used
by posting and deletion. It includes the incoming reply in the decision,
although the row is inserted later in that transaction. A failed insertion
rolls back the count and timestamp update. With a limit of three, the first
two ordinary replies can bump; the third cannot. Deleting replies can restore
eligibility, but the next incoming reply must still leave the count below
three. Sage never bumps an ordinary thread.

Full/tail thread JSON, board JSON and catalog JSON/HTML use the surviving
counts already present in their coherent snapshots. The bump-limit flag is
set at or above the limit, except on sticky or permaage threads, following
`imgboard.php:1036-1073` and `catalog.php:149-152`. Tail responses retain an
explicit zero; full responses omit a false flag. Deletion and policy changes
continue to change body ETags when the representation changes.

Lifetime metadata remains available for the current reply-admission ceiling.
It no longer decides bumping or bump-limit indicators. That admission policy
still needs source matching. [Persisted permaage/permasage controls](thread-bump-flags.md)
now extend these rules, with their own authority and migration coverage.
[Age-based suppression](source-bump-age.md) uses the request-start and OP clocks.
[OP self-bumps](source-op-bumps.md) use private peer matching and strict intervals.
Board-specific spam rules remain unfinished.
[Image-limit exclusions](source-image-limits.md) have separate verification.
These ordinary/sticky rules do not establish full bump-policy parity.

The database regression exercises real public-role writes, below/at/above
cutoffs, sage, deletion, sticky and board-policy transitions, ETag changes,
all four JSON representations, catalog HTML, and eight concurrent replies
at a one-reply cutoff. Existing coherent snapshot tests retain their
commit barriers and healthy blocked-reader witnesses. Domain tests cover
the arithmetic boundary, zero limits, sticky/sage and large counts.

Six-theme desktop/mobile fixture checks cover deleted replies and a sticky
thread above the limit. The two reviewed catalog snapshots change the
deleted-reply marker and include the added sticky fixture. No other baseline
is intentionally changed. Local PostgreSQL is unavailable; persisted and
concurrency results require current-head CI. The original count-only slice
changed no schema, grants, dependencies, processing authority or deployment
settings; the later flag slice has the additive migration described above.

Local checks passed all 17 domain tests, 35 public library tests, all-target
Clippy with denied warnings, 118 theme tests, 39 media/interaction cases,
ten public-state cases and three base visuals. The first catalog run failed
only the two expected screenshot comparisons; both captures were reviewed
before their scoped update, then the full theme suite passed without updates.
