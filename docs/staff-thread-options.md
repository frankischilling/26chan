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

The rewrite exposes one option per authenticated POST form. A forged protected
option returns 403 rather than being silently ignored as part of a larger
source options submission. Current allow/deny scope applies to every rank.
These request and authority replacements use the existing staff security
boundary in [staff operations](staff.md). Full source Thread Options and action
log presentation remain unfinished, including grouped submissions and sticky
ordering. The account/board/target audit is not the complete source action-log
record or interface.

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
attachments, private-board media, grouped/sticky options or the staff extension
and action-log interfaces in issue #222. Whole-source behavior and frontend
parity remain the completion target.
