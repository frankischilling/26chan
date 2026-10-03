# Staff image spoilers

The report queue provides **Spoiler image** and **Remove image spoiler** on
boards whose source `SPOILERS` setting is enabled. The imported
`comment_spoiler_cleanup` policy already represents that setting. Janitors,
moderators, managers and administrators can act within their current board
scope. The controls work as normal POST forms with scripting disabled.

The action changes a post's image-spoiler state without changing its displayed
subject, comment, filename or file availability. It also works on text posts
and retained archives. A removed file stays removed. A deleted post or thread,
an expired archive or a missing target returns 404. A disabled board or invalid
action returns 400. Existing Origin, Fetch Metadata, CSRF, live-session and
ten-minute recent-authentication checks apply.

## Source evidence

The supplied source revision is `545b7812d1849f7958d914950c91fdbbe38f6b22`.
The selected routines are `adminToggleSpoiler` and `admin_toggle_spoiler` in
`admin.php`, plus the original rank/scope helpers in `lib/auth.php`. Their
SHA-256 values are pinned in
[the extractor](../scripts/extract-staff-spoiler-reference.py).

The source stores the state as a reserved `SPOILER<>` subject prefix. It changes
that prefix, logs action mask 129 or 130, and rebuilds the root thread or archive
only when the state changes. A repeated request returns success without an
update, audit or rebuild. It selects posts without requiring an attachment.
The Rust representation retains those state transitions while keeping the
subject as display text. Changed actions append `spoiler` or `unspoiler` to
the existing account/board/target audit and advance thread modification time.
Repeated requests succeed without either side effect.

The extractor invokes the exact source endpoint in separate bounded PHP
processes, including its terminal responses. Synthetic boundaries stand in
for authentication, database rows/writes and thread rebuilding. It produces
[272 cases](../apps/staff/tests/fixtures/staff-spoilers.json) covering four
ranks, board scope, policy, both states, both requested states, OP/reply and
live/archive targets, missing posts and missing flags. It does not connect to
an original database or prove original page rendering.

The source lets moderator-or-higher ranks bypass its endpoint's janitor board
check. The rewrite applies current allow/deny scope to every rank under the
existing [staff authority boundary](staff.md). Authenticated POST forms and
standard application errors replace the source GET/cookie mutation and plain
numeric responses. These replacements belong to security exception E-016.

## Database and upgrade

Apply migration `0078_staff_image_spoilers.sql` before starting the updated
staff binary. It adds `content.posts.image_spoiler`, backfills it from existing
attachment flags and leaves posts without attachments false. It preserves
existing subject strings, attachment metadata, deleted-file markers, private
records and privacy policies. Future attachment inserts and flag changes
synchronize the post state through a private owner trigger.

Of the application runtimes, only `board_staff` can execute
`content.set_post_image_spoiler`. The existing NOLOGIN attachment owner receives
the two specific state-update columns.
Public, staff, authentication, media, reader, intake and monitoring runtimes
cannot write either representation directly or execute the trigger helper.
The setter retains the owner's existing public-content visibility boundary;
private `/j/` attachment and management workflows remain unfinished.

The setter locks board, thread and post in order, checks enabled policy after
the board wait, and checks archive availability after the thread wait. It
returns whether state changed. The application holds current authentication
locks, writes the audit in the content transaction, then rechecks authorization
deadlines before committing. Cancellation or audit failure rolls back the
state and modification time together. Concurrent requests for the same state
produce one change and one audit.

The new [populated upgrade qualification](../scripts/test-staff-spoiler-migration.sh)
runs migrations as the actual migrator login. It retains historical plain,
spoiler, removed-file and text posts, checks old function definitions and
privacy policies, and exercises distinct public and staff connections. The
fresh bootstrap guard checks new columns, execution grants, owner identity,
security-definer settings and the historical false default.

## Qualification and remaining work

The persisted staff attachment suite compares the 144 scoped source cases
against real handlers and saved state. It also tests direct runtime denials,
all-rank scope denials, CSRF/session rejection, concurrent duplicates, observed
audit-lock cancellation, expired archives and deleted targets. The media
reader continues to hide removed files after an unspoiler action.

The staff browser workflow exercises both controls after WebAuthn login,
script-free submission, conditional thread JSON, public spoiler rendering,
unchanged-request validators/audits and stale-authentication denial. Existing
staff media cookie/referrer and opener checks remain in that workflow.

The browser public server uses the existing isolated development profile with
the real intake service and reader. Public, staff and media have distinct
loopback hostnames, so media requests cannot carry either application's cookies.
Staff requests and Chromium wire checks require no referrer; public media
requests may carry only the public origin. The fixture uses trusted synthetic
pixels and no uploads or decoder jobs. Its intake directory is removed only
when empty and verified beneath the owned workspace directory.

The first combined local browser launch ended without a result; PostgreSQL
required restarting afterwards. A separate run exposed a test configuration that disabled
public image markup; the workflow now uses the real intake and reader instead.
That service startup also rejected the newly cloned qualification database's
default public privileges. The owned clone now has explicit connection grants
and no public database privileges. All three service readiness probes pass.
These fixes retain production permission checks, browser assertions, retries
and deadlines. Incomplete and failed-run logs remain local.
The popup request observer also required a correction because initial popup
navigation has no frame yet. The observer now records public-page requests
directly, keeping staff and popup referrer checks strict.

Local qualification passes all 75 staff tests and both Chromium staff
workflows, plus strict workspace Clippy across all targets/features, the
workspace binary/example build, fresh role bootstrap and populated upgrade.
The source fixture regenerates exactly. The browser run took 7.3 minutes in
WSL; its two test bodies took 24.2 and 11.8 seconds. The owned integration
database has 69 successful migration files through version 78. The preceding
version-77 database is unchanged. Full hosted checks remain required for the
published continuation; earlier ordinary-posting CI results apply to their
recorded head in [the posting authority record](staff-posting-authority.md).

Reproduce the source fixture and application checks with:

```sh
python3 scripts/extract-staff-spoiler-reference.py 4chan-old \
  apps/staff/tests/fixtures/staff-spoilers.json --check
cargo test --locked -p board-staff --all-features
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
node node_modules/@playwright/test/cli.js test --config playwright.staff.config.js
sudo bash scripts/test-role-bootstrap.sh
sudo bash scripts/test-staff-spoiler-migration.sh
```

The Rust/database/browser commands require the disposable role environment
described in [development](../README.md) and [staff operations](staff.md).
The source command additionally requires the supplied ignored checkout and
the fixture's recorded PHP runtime. CI consumes the derived fixture; it does
not require the ignored original source.

This continuation does not finish the original staff extension or action-log
interface. Ordinary staff attachments, move/copy/archive management, private
board media and the remaining actions in issue #222 still require work. Public
upload spoiler-policy enforcement and complete source image presentation
remain tracked with the posting/media issues. Whole-source behavior and
frontend parity are still the completion target.
