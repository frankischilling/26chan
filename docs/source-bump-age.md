# Source age-based bump cutoff

[Issue #120](https://github.com/frankischilling/26chan/issues/120) covers the
supplied `imgboard.php:6627-6653` age rule. `PERMASAGE_HOURS=0` disables it.
Otherwise an ordinary reply does not bump when request-start Unix seconds
minus the configured hours times 3600 is at least the OP's creation seconds.
Equality suppresses bumping. A future-dated OP has not reached the cutoff.

The source obtains `$time` from `$_SERVER['REQUEST_TIME']` in `new_post`
(`imgboard.php:4900`). The public middleware captures a server-owned clock
before form collection, password hashing and database waits. It does not
accept clock authority from form fields, headers or existing extensions.
Both posting aliases and both supported form encodings pass that clock into
the store. Internal callers without an HTTP request capture their entry time.

The store reads the OP post's timestamp and the current board policy under the
posting/deletion board lock. The separate thread creation and bump timestamps
do not decide age. Both times are reduced to whole Unix seconds, and the
domain arithmetic uses a wider integer to avoid overflow. A request admitted
before the cutoff can still bump after a lock wait; a later request cannot.
The board policy is the committed policy read after acquiring the lock.

Sticky or permasage still prevents every bump. Permaage overrides age, sage
and the count cutoff, but not sticky/permasage. Age suppression does not reject
the reply, close the thread, set permasage, change the count-based `bumplimit`
indicator, or bypass existing reply/image/archive admission. Counts and
modified timestamps still advance on accepted replies. Later insertion
failures roll back all of those changes.

## Operator policy and upgrade

Apply migration 0023 after 0022 and before deploying the new binary. It adds
`content.boards.permasage_hours`, a required integer from zero through
2,147,483,647 with a zero default. Existing content and policies are retained.
Public and staff runtime column grants exclude policy mutation; only the
operator/migration identity configures it. Board row locks remain available
to both runtimes. The field is not added to public JSON.

The supplied global configuration defaults to zero (`global_config.ini:130`).
Active board overrides are `his:168`, `news:48`, `qa:168`, `qst:120`,
`toy:336` and `vr:336`, from the corresponding board files at lines 8, 13, 28,
55, 11 and 8. The `tg` and `test` declarations are commented out. The migration
does not infer that an existing deployment's board names request those
source settings. Operators recreating that snapshot must apply the relevant
values as part of its board configuration. For one explicitly selected board:

```sql
BEGIN;
SELECT slug FROM content.boards WHERE slug = 'news' FOR UPDATE;
UPDATE content.boards SET permasage_hours = 48 WHERE slug = 'news';
COMMIT;
```

An older binary can read this additive schema but ignores the new age policy.
Rolling back the binary therefore changes bump behavior on enabled boards.
Record that choice before rollback; do not drop policy or historical data.
The request clock relies on synchronized host time, as other timestamp-based
application rules do. Posting timestamps themselves still use database clocks;
matching every source timestamp assignment is separate work.

## Verification

Domain tests cover equality, adjacent seconds, future OPs, zero, source board
values, extreme integer inputs and every flag/count/sage combination with and
without age suppression. The public middleware test delays body collection
and checks that neither headers nor a pre-existing extension supply its clock.

`apps/public/tests/bump_age.rs` uses actual public-role writes with operator-owned
fixtures. It checks persisted timestamps/counts, independently different OP
and thread clocks, fractional seconds, both posting aliases and encodings,
unchanged JSON/HTML indicators, closed-thread denial and late-insertion rollback.
An ordinary no-JavaScript request crosses the cutoff while its streamed body
is being collected and must still bump using its original request clock.
The lock-wait case witnesses a blocked public backend using PostgreSQL's lock
graph before committing an operator policy change across the time boundary.
It verifies the waiting request's original clock and the next request's cutoff.

`scripts/test-bump-age-migration.sh` creates a checked disposable database,
applies migrations through 0022, seeds historical content/audit records and
upgrades to 0023. It exercises disabled defaults, history preservation,
operator updates, invalid bounds, rollback, actual public/staff write/schema
denials and healthy read/lock controls. CI runs this alongside existing
integration, containment and recovery checks.

Local domain and public library tests, all-target/all-feature Clippy and Bash
syntax checks passed. PostgreSQL is unavailable locally; the new persisted
and upgrade tests require complete current-head Linux CI before merge. No
visual baselines or production media authority change. OP self-bump rules,
conditional spam policies, source reply admission, full board configuration
and production deployment/recovery qualification remain unfinished.
