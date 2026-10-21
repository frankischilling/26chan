# Duplicate-comment admission

The source checks equal comments from the same host on the same board before its
ordinary posting timers. The candidate timestamp must be strictly newer than
request time minus 300 seconds. It uses the greatest timestamp, including future
timestamps; the exact five-minute boundary is allowed. PHP treats a zero maximum
as false. Quiet rejection and staff exemptions precede this check. VIP and Pass
membership alone do not exempt a poster.

The extracted branch and `sec2hms` formatter are pinned in
`fixtures/comment-duplicate-source.json`. Run:

```
python3 scripts/extract-comment-duplicate-reference.py ../26chan-reference --check
php -n scripts/test-comment-duplicate-source.php
cargo test --locked -p board-domain comment_duplicates
```

The PHP oracle covers 1,260 synthetic cases for role, quiet rejection, meta-board
settings, name/comment truthiness and temporal boundaries. Four Rust tests pass,
including comparison against every oracle result. The oracle supplies scoped SQL
candidates; it does not establish the original deployed database collation.

Issue #213 stays open while database, browser and remaining source-path
qualification continues. Existing rendered HTML must not be substituted for
source comparison data.

## Private history implementation

Migration 0243 adds versioned SHA-256 comparison keys in a private table. The
key includes the source projection version prefix and compares exact bytes;
the original deployment's SQL collation remains unknown. The public and staff
roles can ask for a remaining duration but cannot read raw keys. Registration
requires matching posting history inserted in the same transaction. Historical
rows with unknown projections remain unknown.

The writer computes the projection before consuming its prepared comment and
shares the same word-filter random outcomes with display preparation. It checks
before ordinary posting timers, including for VIP and Pass users. Independently
verified staff authority selects the janitor timer path. Accepted drawing posts
record the later annotation digest. Image posts on `/p/` and `/test/` still lack
qualified EXIF provenance and are excluded from this new comparison path.

Archive does not remove comparison history. Post/thread deletion does, while
file-only deletion leaves it intact. Lookup uses the actual post timestamp.
The dedicated retention command removes at most 1,000 rows per transaction and
16 batches per invocation, only after request, post and capture clocks are all
older than one hour. History capacity is bounded and exhaustion fails closed.

Local qualification passed the real-writer duplicate test, 25 staff attachment
cases and 10 user-thread quota cases. The owned upgrade script passed rollback,
forward-only history, actual retention-login expiry, wrong-role refusal and
restore checks. Strict domain/store/staff all-target/all-feature Clippy passed.
The browser case is written but has not run locally because the pinned Chromium
is unavailable. Hosted CI and remaining posting paths still need qualification;
issue #213 remains open.


Staff OP quota now runs before terminal content filters, matching the original
call order. The preliminary identity read does not issue posting authority.
Quota refusal revalidates session, CSRF, recent authentication and board access
under locks, including private boards, then rolls back. Accepted posts still
require the independent body-bound proof and the later quota check. The existing
public-board-only filter refusal validator is unchanged.
