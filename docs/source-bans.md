# Source bans and warnings

Issue [#220](https://github.com/frankischilling/26chan/issues/220) covers the
restriction system. This branch adds board/global bans, warnings, expiry and
unban, transactional posting enforcement, staff forms and private public status.
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

`/banned?board=BOARD` returns only the requesting transport or established
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

## Remaining implementation

Report admission and account/Pass flows still need their separate source rules
and private identity binding. The source report check is not the posting policy:
it counts active matching board/global rows without the posting expiry and
warning precedence. Thread-ban batches, reason/length templates, post notices,
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
store, public HTTP and staff HTTP restriction binaries against its socket-only
database. The ordinary workspace CI runs those binaries and the migration check.
Focused runtime results and hosted qualification must be recorded separately;
they do not establish complete source parity or media containment.

## Local qualification

The October 11, 2026 owned WSL/PostgreSQL 16 run passed:

- `cargo fmt --all -- --check`.
- `cargo clippy -p board-store -p board-public -p board-staff --all-targets --all-features --locked -- -D warnings`.
- `RUN_RESTRICTION_RUST_TESTS=1 bash scripts/test-source-restrictions-migration.sh`:
  the migration checks, 13,840 SQL/source decisions and the three runtime test
  binaries passed. The tests cover real public admission and direct-insert
  refusal, scope and automatic identity, expiry auditing, contended issuance,
  private/escaped HTTP status, warning viewing and staff authorization.
- `cargo test -p board-domain --test ban_source --locked`: the Rust policy
  matched all 13,840 independently recorded PHP outcomes.
- The extractor's `--check` against the pinned source passed with PHP 8.3.6.
- `git diff --check`.

Guest clock corrections initially interrupted the vendored OpenSSL build. The
successful local wrapper temporarily paused NTP and restored it on exit; it
did not alter source, dependency versions or test assertions. The disposable
database was stopped and removed after success. These focused checks do not
replace full current-head hosted CI, browser comparisons or the remaining
implementation above. The PR remains draft while that work continues.
