# Staff perma-age and Undead options

The report queue provides perma-age controls for Managers, Administrators and
moderators with global developer permission. Undead controls are available to
moderators and higher ranks. A developer flag does not raise a janitor's rank.
Every action also requires current permission for the target board. The forms
work without JavaScript and use the existing Origin, Fetch Metadata, CSRF,
live-session and ten-minute recent-authentication checks.

Both options apply to existing live threads. Reply IDs and deleted or missing
threads are rejected; archived targets return 400. Archived reports retain
their state display and omit thread-option controls. A valid submission updates
modification time even if the value is unchanged, but never bumps the thread.
Only an actual state change appends an audit. The option, timestamp and audit
share the same transaction and current-authority checks.

## Source evidence

The supplied revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.
`adminvalid` requires moderator rank for Thread Options. `adminopt` accepts
perma-age when `has_level('manager') || has_flag('developer')` is true. Its
unqualified flag helper requires global scope, including the separate
`noboard` denial. Lower ranks preserve the saved perma-age value. The manager
guards around Undead are commented out in the supplied source: the actual
code assigns that option for every rank allowed into Thread Options.

[The extractor](../scripts/extract-staff-thread-options-reference.py) pins
`admin.php` and `lib/auth.php` hashes and executes their selected permission,
assignment and audit routines in bounded PHP processes. It produces
[512 synthetic cases](../apps/staff/tests/fixtures/staff-thread-options.json)
across all four ranks, developer flags, global scope, `noboard` denial, old and
requested perma-age, and old and requested Undead. Original audit masks use
bit 8 for perma-age and bit 16 for Undead, with no audit when unchanged.
Database calls and feedback are bounded synthetic boundaries. The extractor
does not execute the whole legacy endpoint, its page renderer, sticky ordering
or rebuilds, and does not connect to an original database.

The isolated rewrite actions expose one option per authenticated POST form.
A forged protected option returns 403 rather than being silently ignored as part of a larger
source options submission. Current allow/deny scope applies to every rank.
These request and authority replacements use the existing staff security
boundary in [staff operations](staff.md).
[Grouped Thread Options and numeric sticky ranks](grouped-thread-options.md)
now have a separate implementation whose qualification is pending. Full source
action-log presentation remains unfinished. The account/board/target audit is
not the complete source action-log record or interface.

## Releasing a sticky thread

The source's `adminopt` sets `root=now()` when a sticky thread becomes ordinary;
submitting an already ordinary thread preserves its root. The rewrite applies
that transition to `bumped_at`, using database mutation time after its existing
board/thread locks. Remaining sticky threads still sort ahead of the released
thread. Repeating the action preserves its bump time and adds no unchanged audit,
while the valid submission still refreshes modification and HTTP freshness times.

Archived targets reject Unsticky, matching the source's active-thread selection.
The schema already forbids archived sticky rows; tests keep that constraint and
exercise a representable archived ordinary target. The commented source code
that would clear Undead is not active and is not applied here.

A separate [bounded extractor](../scripts/extract-staff-unsticky-reference.py)
records [32 synthetic cases](../apps/staff/tests/fixtures/staff-unsticky.json):
16 actual releases use `root=now()` and append the changed mask; 16 no-op cases
use `root=root` without an audit. All combinations keep the other four flags.
It executes only the selected assignment branch and audit helper with a recorder,
not the endpoint, database, rank calculation, rebuilds or actual clock operation.
Database tests qualify the corresponding mutation clock and transactional state.
Numeric sticky ranks are documented in the separate
[grouped-options continuation](grouped-thread-options.md).

## Isolated close and perma-sage actions

Close/Reopen and Perma-sage/Undo perma-sage change only their selected flag.
They preserve bump time and all sibling options. A valid no-op refreshes the
modification and HTTP freshness clocks but adds no audit. An actual change adds
one audit in the same transaction. Archived targets reject all four actions,
including forged or stale Close submissions whose controls are already hidden.

This follows the unchanged-mask rule in `log_thread_opts_action` for the existing
isolated forms. It does not reproduce ReportQueue's sparse Perma-sage shortcut:
that source request omits other fields consumed by `adminopt`, so it can reset
sibling flags and legitimately produce an audit even when perma-sage was already
set. The rewrite's explicit one-flag actions retain those siblings. The separate
[grouped Thread Options route](grouped-thread-options.md) implements grouped
submission semantics; it does not change these isolated actions.

The [isolated-action audit fixture](../apps/staff/tests/fixtures/staff-isolated-options.json)
executes the pinned audit helper for all 128 old-mask/action combinations:
64 changes and 64 no-ops. Four separate sparse-request examples are hypothetical
prepared outcomes, not executed source request parsing or permissions. They
illustrate why that shortcut cannot be treated as an isolated setter. The
[extractor](../scripts/extract-staff-isolated-options-reference.py) never runs
the endpoint, assignments, archive checks, clock, database or rebuilds.

## Database and operation

Apply migration `0079_staff_thread_options.sql` before starting the updated
staff binary. It grants the existing staff runtime UPDATE on the existing
Undead column and admits `undead`/`unundead` audit actions. It changes no rows,
owner roles, function definitions, table grants or privacy policies. Other
application runtimes cannot update Undead. Rank and board permission remain
account checks in the application, as with other thread options.

The new [populated upgrade](../scripts/test-staff-thread-options-migration.sh)
applies migration files as the real migrator login on a private disposable
cluster. It preserves historical option states, posts, timestamps, deletion
records and audits, compares privacy/function/other grants in both directions,
and exercises all seven runtime connections. Fresh bootstrap independently
requires Undead mutation to be confined to staff. Published migrations 0077
and 0078 retain their checksums. The integration qualification uses a separate
database with 70 successful migration files through version 79; its qualified
version-78 parent is unchanged.

An older binary cannot provide the new controls. Keep the additive schema and
audit history during binary rollback, and review enabled options against the
older binary's behavior. No destructive down migration is provided.

## Qualification and remaining work

The results below describe earlier isolated-option checkpoints. They do not
qualify the new [grouped/rank continuation](grouped-thread-options.md), whose
verification is pending.

Hosted build/test for `a804708` failed on an older public thread-tail
permission assertion that still denied staff Undead writes. The corrected
test exercises a real staff write and rollback, keeps the public write denial,
and checks that both runtimes remain unable to change board policy. That test
and public Clippy across all targets/features pass locally against version 79.
The same hosted run passed media/operations. Windows passed 1,172 checks and
failed one mobile dark-color comparison; its retained browser trace records
`net::ERR_NO_BUFFER_SPACE` for the theme stylesheet. The runner resource
aggregates do not establish the cause of that transport failure. Color
expectations, screenshot baselines and retries remain unchanged. These partial
results do not constitute complete hosted qualification.

The persisted permission suite selects original cases where only one option
changes and compares 512 form submissions through real authenticated handlers.
It checks saved values, current account and board permissions, button visibility,
unchanged-request timestamps and audits, source audit masks, and unchanged bump
times. Additional cases cover explicit all-rank board denials, CSRF rejection,
concurrent duplicate requests, cancellation observed at the audit lock and
archived targets. The source rank/flag predicate is also compared against all
512 recorded cases without a database.

The browser workflow exercises Manager and globally flagged moderator
perma-age forms, scope changes that revoke old sessions, and script-free
Undead controls. It compares conditional public JSON at the image cap,
unchanged audits, advancing modification times and fixed bump times. Existing
WebAuthn, recovery, logout, public rendering and media privacy checks remain.

All 77 staff tests, strict workspace Clippy across all targets/features,
workspace binary/example builds, fresh bootstrap and the populated upgrade
pass locally. The combined browser launch ended after announcing two tests
without a result or diagnostic. PostgreSQL was no longer running and was
restarted through the existing owned-cluster guard. That incomplete log is
retained. Both staff workflows pass in the final run with harmless inherited
cloud, agent and extra database values injected. The final WSL browser run took
7.4 minutes; the two test bodies took 24.4 and 13.7 seconds. Assertions, retries,
deadlines and screenshot baselines are unchanged. The derived source fixture
regenerates exactly, and local documentation links and formatting pass.
The single interrupted synthetic board/account/media fixture was removed only
after checking inactive clients, its database identity and exact workspace
paths. Its intake directory was empty. The qualified parent remains unchanged.
This continuation requires
its own complete hosted checks after
publication. Earlier spoiler and ordinary-posting results apply to their
recorded heads.

The preceding `131c903` hosted build passed Windows visuals and media/operations
but failed at staff browser startup with an intake configuration rejection.
The browser service profile now clears inherited credential names rejected by
intake, including unknown database aliases, while forwarding each service's own
database connection. The runtime configuration checks are unchanged. A real
intake process rejects synthetic Azure, SSH-agent and extra database values;
the profile overrides restore authenticated readiness 200. CI injects those
same harmless values into the staff browser workflow. The failed hosted log
does not identify which setting was rejected, so complete checks for the next
published head remain required.

```sh
python3 scripts/extract-staff-thread-options-reference.py 4chan-old \
  apps/staff/tests/fixtures/staff-thread-options.json --check
cargo test --locked -p board-staff --all-features
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
node node_modules/@playwright/test/cli.js test --config playwright.staff.config.js
sudo bash scripts/test-role-bootstrap.sh
sudo bash scripts/test-staff-thread-options-migration.sh
```

The application checks require the disposable role environment described in
[development](../README.md) and [staff operations](staff.md). Source regeneration
additionally needs the supplied ignored checkout and recorded PHP runtime.
CI uses the derived fixture without the original checkout.

These controls do not complete move/copy/forcearchive, ordinary staff
attachments, private-board media or the staff extension and full action-log
interfaces in issue #222. The grouped/rank continuation still requires its own
qualification and does not establish complete source UI parity. Whole-source
behavior and frontend parity remain the completion target.
