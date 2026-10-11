# Source bans and warnings

Issue [#220](https://github.com/frankischilling/26chan/issues/220) covers the
restriction system. This branch adds board/global bans, warnings, expiry and
unban, transactional posting and report enforcement, staff thread-ban batches,
staff forms and private public status.
The remaining source workflows below keep the issue open. Current-head hosted
checks are required before merge.

## Runtime boundary

Migration 0134 uses the non-login `board_restriction_owner` role. Bootstrap it
before upgrading an existing database. Neither application can assume that role
or read restriction identities, private reasons, capacity or audit tables.
Staff issuance resolves a live post's private transport and automatic-session
identities inside the database. It checks current moderator-or-higher authority,
board access, CSRF, credential presence, session expiry, idle expiry and recent
authentication before and after content locks. Reasons and retained rows have
fixed byte and count limits; exhausted capacity refuses issuance.

The ordinary and staff writers evaluate restrictions after the posting actor
gate and before content, session or media locks. A shared restriction lock lasts
through commit, so concurrent issuance cannot invalidate an admitted write.
An insert trigger checks direct writers too. Refusal writes no post or anonymous
activity. Expired rows visited before a later blocking match are retired and
audited in the refusal transaction. Applications refuse startup when the sealed
restriction catalog has changed.

Staff use `/ban?board=BOARD&target=POST` to issue a restriction. `/ban/unban`
retires a restriction after checking its stored board's current authority.
The report queue links live posts to the form. These forms provide manual
reasons and durations; source template selection and combined post actions
remain unfinished.

`/banned?board=BOARD` returns only the requesting transport or valid stored
automatic identity's public reasons and restriction dates. Its native viewing
form can retire only a matching active warning. Viewing uses a separate HttpOnly,
SameSite cookie and a CSRF proof, requires a same-origin mutation and grants no
posting or deletion identity. Status responses use `private, no-store`, escaped
templates and a policy that denies scripts. The status/viewing contract is
reconstructed because the supplied snapshot omits its controller. Exact original
page styling remains unqualified. HTML posting refusals provide the source's
status links and a three-second Refresh redirect without inline script. Native
JSON refusals retain a fixed plain-text error. Actual browser redirect behavior
and complete source-page comparisons remain unqualified.

Status first runs posting's visited-expiry handling, then shows remaining
matching active rows, including expired rows that still prevent reporting.
Stored automatic equality remains usable for status and warning viewing during
creation-second or seven-day idle reset. Posting keeps its separate `!isNew()`
rule. An unknown or expired capability grants no status or viewing authority.
This native contract lets a user see and acknowledge a warning that blocks
reporting after their transport address changes; it does not claim behavior
from the missing source controller.

## Posting decision

The supplied `imgboard.php:8554` checks active matches for the automatic
`UserPwd` password, Pass and transport address. Password/Pass fields retain the
caller's order; the address field is appended unless a present Pass and the
qualified CAPTCHA bypass both authorize skipping IP bans. A supplied browser
checkbox or unauthenticated account string establishes neither fact.

Warnings have a nonzero end timestamp at or before their start. They block
before the expiry, board/global scope and entitlement exceptions are checked.
Consequently a warning from another board still requires viewing the warning.
Other finite restrictions expire at `end <= request time`; a zero end denotes
an indefinite restriction. The source retires only expired rows visited before
the first warning or applicable ban. Its result order is not specified by SQL.

Pass matches skip template 124 after warning and expiry handling. A verified
user skips an address match issued by exactly `Auto-ban` when the complete
persisted reason contains case-sensitive `Proxy`, including the private part.
Other address matches and automatic-password bans remain applicable. The
rewrite must derive these facts from server authority, retain private reasons
behind staff access and define a stable database order explicitly.

`imgboard.php:4889-4891` gets the posting password from `UserPwd`. At line 5288
the source permits password matching only when `!isNew()`; it does not use a
submitted recovery password for this decision. The existing private automatic
identity records provide that equality without retaining the cleartext cookie.
Historical recovery-password hashes cannot be used to invent this identity.

`admin.php:353` calls the batch operation `cpban`. It bans the OP globally,
bans each distinct reply address except the OP address globally with a
separate duration/reason, then deletes the thread. The supplied posting
`check_for_ban` receives a thread ID but never uses it. A restriction confined
to that thread would replace the actual source behavior.

## Thread-ban batches

Migration 0136 implements `/ban/thread`. The staff form appears only for a live
OP with source Manager-or-higher authority or a global `threadban` flag and the
Moderator floor. The flag also requires no-board access, as source
`has_flag('threadban')` does. Board access and `/j/`'s Manager floor still apply.
Issuance checks the real session, credential, CSRF and recent authentication in
SQL, including a fresh check at commit.

The OP gets one global ban; each other distinct address gets one global ban
with the separate reply reason and duration. Replies sharing the OP's address
do not acquire another ban. Durations are whole days from 0 through 9999;
zero days produces a warning. Day fields preserve PHP's numeric-prefix,
fraction truncation and exponent casts before checking that range. Empty or
non-numeric strings cast to zero; raw day fields over 128 bytes are refused.
The defaults are three days for the OP and zero
for replies, with the source's off-topic reason text. Source `ban_post` writes
host and Pass equality but never `UserPwd` password equality. The native batch
therefore captures only the private host digest. It grants no Pass exemption
and does not invent an automatic-session ban.

The store issues all bans, deletes the thread through the existing deletion
path and writes the moderation audit in one transaction. A private deferred
trigger refuses commit if deletion is incomplete or staff authority has expired.
The promise's session and CSRF hashes disappear before a successful commit;
applications cannot read or write the promise table. Capacity refusal rolls
back the whole batch. Threads exceeding 10,000 live posts or 1,000 distinct
addresses are refused before issuance.

Actor locks are taken in stable stripe order before the restriction lock.
The transaction then locks the board, thread and live posts and recaptures
their identities. A newly arrived address outside the captured stripes causes
rollback and a fresh attempt, with at most three attempts. It never acquires a
new actor stripe while holding the restriction lock.

These are explicit security changes: a forged reply target is rejected; the
source function itself relies on its OP-only form. Sticky thread deletion
requires an Administrator, and any deletion failure rolls back the bans; the
source sends an unchecked deletion RPC after writing them. Missing private
history causes refusal. Native grouping chooses the oldest live representative
of each address and a stable address order; the source SQL does not specify
either. Source Pass binding, the saved post-JSON ban snapshot, database defaults
and the original modal presentation remain unfinished or unqualified.

## Report decision and identity binding

Migration 0135 implements the separate active-ban count in
`modes/report.php:170-195`. GET remains advisory and skips bans; POST evaluates
them after the target and duplicate/cooldown/quota checks. An active matching
board/global row blocks reports even when its date has expired. Warnings obey
their stored scope here. Reports do not retire expired rows, apply template 124
exceptions or skip verified proxy bans. The source adds Pass equality only when
both Pass presence and CAPTCHA bypass are qualified; it always keeps address
equality. The rewrite has no trusted Pass provider and grants no such exemption.

The report identity type carries two private HMACs derived from the same
verified peer and secret key. Report quotas keep their existing digest; bans
use the independently derived posting digest captured during issuance. No
historical equality is inferred and neither digest enters a response. Valid
stored automatic credentials can match bans during their creation second or an
idle reset because the report source uses `getPwd()` without posting's
`!isNew()` condition. Fresh credentials invent no previous equality.

The store binds this context before report board, quota or session locks and
holds a shared restriction gate until commit. SQL admission checks bans before
inserting reports, evidence, membership, snapshots or anonymous activity. A
report insert trigger checks again and consumes the context. Context is bound
to the current transaction, board, target and report digest, so another report
needs a fresh binding. Direct legacy admission without context refuses when
any active restriction exists. When the restriction table has no active rows,
the shared lock makes that empty-state decision safe for historical direct
fixtures. Only the migrator can import historical reports without this context.

The upgrade checks the reviewed admission definitions before replacing them,
preserves existing report rows and seals the new guard and admission bodies.
Disabling the guard, removing admission enforcement or changing its privileges
invalidates readiness. The report popup uses the source wording and an escaped
local status link; exact original rendering remains unqualified.

## Remaining implementation

Report range/ASN restrictions and account/Pass flows still need their source
rules and authoritative identity/entitlement binding. Reason/length templates,
post notices, other combined post actions,
the public recent-ban sample, janitor requests and special restriction types
remain unfinished. Concurrent issuance, revocation and viewing need broader
runtime qualification before this issue can close.

The source form also supports templates, warning/finite/indefinite duration,
configured public notices, deletion, file deletion, all-poster deletion,
reply-only deletion, thread movement/archive/close/permasage and manager-only
special restrictions. The public recent-bans task filters active public
templates and omits `/test/` and `/j/`. These flows need implementation and
independent source comparisons, not inferred behavior from the posting helper.

The supplied snapshot has no public `/banned` controller, warning acknowledgement
or appeal template, authoritative ban-template rows or Pass provider. Their
contracts must be identified separately from executed source behavior.
Original media preservation and unavailable transfer modes retain their
existing issue boundaries. No unsafe original-file publication is introduced.

## Comparison boundary

`scripts/extract-ban-enforcement-reference.py` executes the unchanged pinned
source function through PHP. Only exact active synthetic database matches,
request time, salt and recorded expiry writes are stubbed. A fresh owned empty
directory confines thumbnail cleanup. The fixture includes field and row ordering, warning
precedence, expiry boundaries, scopes and the two qualified exceptions.
`crates/domain/tests/ban_source.rs` compares the pure Rust policy with every
executed result. `scripts/ban-reference-sql.py` feeds the same independently
recorded outcomes to the database policy; the disposable upgrade script compares
all 13,840 results. These comparisons do not prove identity binding, transactions
or page fidelity.

The database script checks populated upgrades, rollback on prior visibility
drift, actual role denials, private target resolution, issuance/unban/warning,
capacity and catalog changes. With `RUN_RESTRICTION_RUST_TESTS=1` it also runs the
store, public HTTP and staff HTTP restriction binaries plus report-target
regressions against its socket-only database. Ordinary workspace CI runs those
binaries and the migration check.
Focused runtime results and hosted qualification must be recorded separately;
they do not establish complete source parity or media containment.

`scripts/extract-report-ban-reference.py` separately executes the unchanged
`report_check_ip` function. Synthetic prior-report counts are zero, ranges are
allowed and password/Pass facts are explicit boundary stubs. Its 3,456 cases
cover GET/POST, active state, board/global scope, dates, template 124 and identity
clause selection. `scripts/report-ban-reference-sql.py` compares every result
with the SQL report policy. The migration script also checks populated report
preservation, definition-drift rollback and guard/admission drift. These checks
do not qualify ranges, Pass authentication, quotas or original rendering.

`scripts/extract-thread-ban-reference.py` executes unchanged `ban_post` and
`cpban` from the pinned `admin.php` through PHP 8.3.6. Its 107 traces use synthetic
post rows and exact SQL-boundary stubs, a fixed clock and DNS result, and a
recorded deletion RPC. Of these, 88 valid OP cases cover duplicate addresses,
OP-address replies, separate reasons, durations 0, 1, 3 and 9999, and source
coercion of numeric prefixes, fractions, exponents and non-numeric strings. PHP
primitive casts are also recorded independently beside unchanged function
execution; the Rust day parser compares both fields in every trace. The SQL
comparison checks the actual stored native bans and their audit rows against
those independent results. Invalid reasons/durations and missing targets are
rejected; the source's forged reply case is recorded as the security difference
above. The comparison does not establish real MySQL defaults, unspecified group
order, Pass authentication, post JSON, successful RPC deletion or source UI.

The store runtime checks use actual posting identities and live staff sessions.
They cover private capacity exhaustion, incomplete-deletion rollback, authority
expiry at commit, concurrent reply recapture, duplicate-address grouping, global
posting refusal and absence of invented automatic-password equality. Staff HTTP
checks cover form visibility, global flag scope, Manager authority, sticky
Administrator authority, credentials, CSRF, recent authentication, invalid and
reply targets, separate reasons and successful atomic deletion. The migration
check separately rejects oversized post and identity batches without retaining
bans or promise credentials. Hosted CI must qualify the resulting commit.

The subsequent October 11 report run passed strict Clippy, formatting, both SQL
source comparisons and all three updated runtime binaries. The store fixture
qualified reporting scopes, active expired rows without retirement, source
quota precedence, free-text/categorical refusal, automatic equality during
creation-second reset and changed address, missing/mismatched SQL context and
successful atomic report/evidence/membership/snapshot/registration. Public HTTP
qualified advisory GET and both accepted POST form shapes, private uncached
refusal, the safe status link and no refusal cookie or saved report. The
historical session-report upgrade and dump/restore script also passed both
profiles after making the expected lookup grant depend on whether restriction
support is installed. The snapshot upgrade and dump/restore passed all three
transfer-order profiles. That historical fixture stops its setup at 0133 so it
does not apply a dependent later migration before its target. Hosted checks must
qualify the resulting commit separately.

The subsequent report-target regression check covered free-text and categorical
POST, unknown board names, zero/negative targets and board-policy precedence.
The binder records their identity context without authorizing the target;
ordinary target admission supplies the existing rejection. Native status/viewing
checks covered creation-second and idle retained capabilities, changed address,
unknown/expired credentials, visible active expired rows and one viewing audit
event. The HTTP check registered its session through a real accepted report and
required both that credential and the separate viewing cookie to acknowledge
the retained warning. These checks passed with the migration/source comparisons
and the restriction runtime binaries.

## Local qualification

The October 11, 2026 owned WSL/PostgreSQL 16 run passed:

- `cargo fmt --all -- --check`.
- `cargo clippy -p board-store -p board-public -p board-staff --all-targets --all-features --locked -- -D warnings`.
- `RUN_RESTRICTION_RUST_TESTS=1 bash scripts/test-source-restrictions-migration.sh`:
  the migration checks, 13,840 posting and 3,456 report SQL/source decisions,
  88 valid thread-ban comparisons and four runtime test binaries passed.
  The tests cover real public admission and direct-insert
  refusal, scope and automatic identity, expiry auditing, contended issuance,
  private/escaped HTTP status, warning viewing, report-target errors, atomic
  thread-ban deletion, recapture and staff authorization.
- `cargo test -p board-domain --test ban_source --locked`: the Rust policy
  matched all 13,840 independently recorded PHP outcomes.
- `cargo test -p board-domain --test thread_ban_reference --locked`: both day
  fields matched all 107 PHP cast traces. The existing drawing-submission
  integer tests also passed after sharing the same parser.
- The posting and thread-ban extractors' `--check` against the pinned source
  passed with PHP 8.3.6.
- `git diff --check`.

Guest clock corrections initially interrupted the vendored OpenSSL build. The
successful local wrapper temporarily paused NTP and restored it on exit; it
did not alter source, dependency versions or test assertions. The disposable
database was stopped and removed after success. These focused checks do not
replace full current-head hosted CI, browser comparisons or the remaining
implementation above. The PR remains draft while that work continues.
