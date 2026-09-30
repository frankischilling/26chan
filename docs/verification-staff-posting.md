# Staff posting verification

These checks use owned localhost PostgreSQL 16 roles and synthetic records.
They qualify the implemented text-post slice. They do not establish original
private authentication behavior, physical authenticator protection, staff
attachment posting or production launch readiness.

## Database and HTTP checks

Fresh bootstrap applied all 42 migrations in a disposable Unix-socket cluster,
retained historical fields and clocks, checked new nullable labels separately,
and checked the new NOLOGIN owner and runtime grants. The cluster was stopped
and removed. A second owned database was built from the current migration bytes
for application tests; it does not reuse the earlier fixture's unpublished
migration checksum.

The five posting integration cases passed with actual authentication, staff
content and public logins. They cover all six saved labels, default assignment,
highlight denial, CSRF and origin checks, forged fields, public cookies/headers,
raw table and function denials, stale and expired sessions, exact audit writes
and absence of consumed authorizations. Full/tail/index/catalog JSON passed on
both listeners. HTML, updater and preview responses retain the saved labels;
staff queues escape content and display the badge. Forged public receipt IDs
are denied. Staff insertion preserves the prepared display name while excluding
private trip suffixes.

Seven prepared fields are independently changed before insertion; each mismatch
is rejected. Concurrent consumers produce one post and one audit entry from one
authorization. Thirty-four concurrent issuers admit exactly 32 records and deny
two; expiring those records permits a healthy new authorization. A healthy
session-row lock is observed through PostgreSQL's blocking graph before the
absolute deadline passes. The waiting consumer then denies the post and audit.

One local run exhausted its observation interval without seeing that clock
deadline. The failure remains unexplained. The fixture now reports both deadline
and observed database clock on failure. A subsequent concurrent run returned
the actual role's `55P03` lock timeout at the same two-second boundary. The
expiry fixture now assigns a one-second deadline, below the unchanged two-second
runtime lock timeout. It still observes a healthy row lock, waits for the
database clock to expire the unchanged session row and requires denial afterward.
The earlier clock-observation failure's cause remains unknown.

The operator-revocation regression initially returned `55P03`: a waiting post
held a proof or session lock while the operator held the account lock and
needed to delete that session. The unpublished migration now orders account,
session and proof locks consistently. Both issuance and consumption pass the
regression with the actual runtime logins: the operator deletes the session
within its 250-millisecond test limit, and the waiting request returns `28000`
without creating a post or audit entry.

The broader staff suite passed 37 tests. Eight public asset-route tests also
passed, including the nine pinned GIFs, exact hashes and MIME, GET/HEAD behavior,
write denial, API-listener absence and the complete fixed-path CSP. Workspace
Clippy passed with all targets, all features, locked dependencies and warnings
denied. The final focused run also passed all five posting cases, actual
runtime identity boundaries and workspace Clippy after the lock-order and
startup corrections. The complete workspace rerun passed 542 tests with zero
failures and zero ignored cases.

The first full workspace run stopped at the monitoring fixture's owned-cluster
marker guard. The new private test helper had omitted `BOARD_TEST_CLUSTER`.
The helper now verifies the running PostgreSQL data directory against the
recorded disposable cluster before exporting that marker. The guard and test
assertions remain unchanged. The complete rerun passed after that verification.

## Native browser checks

All 78 combined updater, quote-preview, filter-settlement and ID-action cases
passed. The regenerated worker is 247488 bytes and passes the source and
thread-control bundle checks. Negative recipes cover inconsistent badge labels,
classes, titles, dimensions and icons, arbitrary density URLs, duplicate badges,
comment placement, added controls and mixed ordinary identities.

Actual Chromium local previews retain all five displayed labels in both the
source module and release bundle. At density two they load each fixed density
asset; founder uses its available ordinary icon. Original controls are omitted
from the copy. Group selection passes click, keyboard, live-post and page
lifecycle checks, preserves administrator post styling and offers no ID-count
tooltip for staff labels.

The first expanded WebAuthn run stopped at startup: checking a cross-boundary
function by textual signature required usage of a deliberately inaccessible
schema. The check now inspects that function's catalog OID without granting
schema access. A following harness run exposed empty optional public key values;
the public process now receives those optional sources as absent, while the
staff process still excludes them. The actual WebAuthn form/updater scenario and complete image-CSP browser
regression passed with these corrections. The final rerun also passed against
the rebuilt database after the lock-order change. It covers real virtual
WebAuthn enrollment/login, a script-free moderator post, invalid assignment
rollback, assignment and role session revocation, an administrator-highlighted
reply, live updater insertion and a local badge preview. The staff flow took
12.5 seconds after the local availability preflight; the complete image-CSP
case passed in 1.6 seconds. No staff traces, screenshots or videos were retained.

## Hosted state

The preceding private-count and flag batch merged as
[PR #184](https://github.com/frankischilling/26chan/pull/184) on September 30 at
`4cd3dc81eefbd621c1a03ea3f2406de489d776c5`. Its tree matches the checked
`687c78528e3c51c42ada5012d30af72f31ece08e` head. All six checks passed: build
36679040274, monitoring 36679040256 and advisories 36679040262.
This record covers local qualification before publication of the staff
continuation. Hosted checks must pass on its published head before merge.
