# Thread bump flags

[Issue #116](https://github.com/frankischilling/26chan/issues/116) adds persisted
permasage and permaage to the existing count-based bump rules. The supplied
`imgboard.php:6627-6653` gives sticky/permasage first priority: either suppresses
a bump even when permaage is set. Otherwise permaage permits a bump despite sage
or the count cutoff. Ordinary replies retain the post-insert surviving-count
rule in [source bump rules](source-bump-rules.md).

Full/tail thread JSON, board JSON and catalog JSON/HTML exclude permaage threads
from the bump-limit indicator, as `imgboard.php:1036-1073` and
`catalog.php:149-152` do. Permasage alone does not change that indicator. The
internal flags are not added as public JSON fields. ETags remain body hashes:
permasage can leave a full JSON response unchanged, whereas permaage changes it
when it removes a true bump-limit field.

## Staff authority

The supplied `admin.php:3518-3630` restricts thread options to existing,
nonarchived OPs and protects permaage changes with manager/developer authority.
This app currently has moderator and admin roles, not the complete original
role hierarchy. Both can set or clear permasage; only admin can set **or clear**
permaage. This is an explicit restrictive mapping, not a claim of role parity.
The server checks that distinction independently of which buttons it renders.

Changes use the existing staff origin, CSRF, current session and recent-sign-in
checks. The store locks the board and thread, rejects archived/deleted/missing
targets, changes the selected flag and modification timestamp, and inserts its
audit action in the same transaction. A flag change does not itself bump the
thread. Public database credentials cannot insert or update either column;
the staff login still cannot read staff identity records. The SQL staff login
is shared by both staff roles, so the admin-only distinction is enforced in the
authenticated application, not by separate per-user SQL identities.

The report queue shows both states, permasage forms, and admin-only permaage
forms. They work without JavaScript. This does not implement the entire
original staff interface or its report/category/role policies.

## Upgrade and rollback

Apply migration `0022_thread_bump_flags.sql` with migration authority before
starting the new public or staff binary. It appends two non-null false-default
columns, refreshes the security-barrier visible-thread view and extends the
audit action constraint. Refreshing the original `t.*` projection also includes
the already-persisted undead column added by migration 0020. Existing view
grants and visibility predicates are retained. Public column-scoped mutation
grants are not expanded.

The migration does not rewrite historical posts, timestamps, counts or audit
rows. An older binary can run with this additive schema, but does not honor the
new flags; assess any enabled flags before a binary rollback. Keep the schema
and audit history when rolling back binaries. There is no destructive automatic
down migration.

`scripts/test-thread-bump-migration.sh` creates an owned disposable database in
the checked development cluster, applies migrations through 0021, seeds old
content/audit data, then upgrades. It checks false defaults, the refreshed view,
historical values, real public-login denials, real staff-login updates and audit
insertion, transaction rollback and the view's security barrier. It removes
only that generated database. CI runs it alongside the existing upgrade and
restore exercises.

## Verification and remaining rules

Domain coverage exercises every flag combination against sage, zero and
boundary limits, and large counts. The actual-public-role bump regression
checks persisted timestamps and all bump-indicator representations across
permaage/permasage/sticky transitions. A closed permaage thread still rejects
replies. The staff database regression checks both directions, moderator/admin
authority, stale and unsupported roles, wrong-board/reply/archived targets,
unchanged rejected state and exact audit records. Public flag INSERT and UPDATE
denials retain healthy positive controls.

The virtual-authenticator browser case adds forged moderator permaage denials,
persisted permasage, role-change session revocation, admin no-JavaScript flag
forms, public cache/indicator checks, and the exact audit sequence. The existing
stale-session probes use the new session's CSRF value after reauthentication.

All 18 domain tests, 35 public library tests and public/domain/store Clippy
pass. All 118 theme, 39 media/interaction and ten public-state cases pass
without snapshot updates. Local staff
compilation cannot reach the crate because vendored OpenSSL needs unavailable
Perl build modules; the installed Git Perl also lacks those modules. Local
PostgreSQL is unavailable. Staff compilation, persisted tests, browser sign-in
and the upgrade exercise require this change's complete Linux CI before merge.

[Image-limit exclusions](source-image-limits.md) have separate implementation
and qualification. Age suppression, OP self-bumps, board-specific spam
rules and source reply admission remain unfinished. These flags do not bypass
closed/archive/reply/image admission or expand processing authority. Production
launch/recovery and full source parity remain separate unfinished work.
