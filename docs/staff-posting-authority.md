# Staff posting authority and peer transport

The pinned source separates authentication from the visible staff badge.
Robot9000 checks an ordinary post even when its author is authenticated. It
bypasses that check only when the prepared Options field is exactly `bypass_r9k`
and the caller has janitor-or-higher authority. A selected staff badge also
bypasses Robot9000. Case changes, surrounding spaces and combined commands
do not match the ordinary-post bypass after Options preparation.

The source first removes every ASCII case-insensitive `sage` occurrence,
then checks `nonoko`, the badge prefix and Robot9000. Consequently
`bypass_r9ksage` requests sage and bypasses Robot9000 for authenticated staff;
`bypass_r9k sage` retains a space and does not bypass it. Ordinary Options do
not invoke badge permission checks or mask the author's name. A `capcode_`
attempt does invoke those checks, even if its resulting badge is empty.
Only moderator-or-higher rank grants the authorized comment limits. The
[Options reference extractor](../scripts/extract-staff-posting-options-reference.py)
compares 2,048 combinations of rank, flags,
global and board scope, and input text with that preparation order.

The early CAPTCHA and range-ban bypass follows local authentication or
janitor-or-higher authority independently of the badge. Other admission rules
remain separate. The pure reference extractor runs these two predicates from
the hash-checked source with synthetic boundary results; it does not run
source authentication, database queries or the Robot9000 plugin. The fixture
contains 224 Robot9000 cases and four early authentication-bypass cases.

Authenticated janitors, moderators, managers and administrators can select
`None` in the staff text form to post with ordinary public identity. These
posts retain tripcodes, thread IDs, private poster counts, geography or a
board flag, sage, OP membership, formatting and deletion passwords. Ordinary
posts run content admission and Robot9000. Badged posts also run content
admission on `test`. Moderator-or-higher rank grants the authorized comment
limit and closed/full-thread exception independently of the visible badge;
archived threads remain unavailable.

The report queue includes a Post link for janitors when ordinary posting is
configured, and keeps posting navigation for higher ranks. Both badge and
ordinary posting use that entry point. The four-rank browser workflow enters
through this link after each login.

`STAFF_POSTER_ID_KEY` supplies the private identity key for staff posts. Use
the same key as the public application's `POSTER_ID_KEY` to preserve thread
labels, private counts, Robot9000 identity and posting history across both
listeners. All staff posting and readiness require this key; the parser still
permits an absent key for read-only startup. It rejects malformed and all-zero
values. Optional `STAFF_COUNTRY_DATABASE` uses the same verified country data
under its staff-specific path. The loader requires an absolute regular file,
limits it to 64 MiB and verifies the country database format before serving.
Startup errors disclose neither the key nor the supplied path. Inherited
public identity variables and unrelated database credentials remain rejected.
Ordinary and badged posting both require an actual listener peer. Production
requires the verified Unix proxy profile below. Visible badge labels and staff
permission checks remain unchanged; see [posting cooldowns](source-posting-cooldowns.md)
for private history registration and the remaining staff timer differences.

The public and staff binaries share the existing bounded HTTP transport and
kernel-verified proxy resolver. Public callers keep their existing interface.
The generic socket, capacity, deadline, cancellation and cleanup tests move
with that transport; the actual public-router Unix identity test remains in
the public application. Windows CI runs the shared transport tests explicitly.

The staff listener optionally accepts `STAFF_PROXY_SOCKET` and
`STAFF_PROXY_UID`. Configure both together. The socket path must be canonical,
absolute and bounded; Unix proxy serving requires Linux. Production rejects
root and invalid proxy UIDs. The server checks the connecting kernel UID
before accepting exactly one `X-Board-Client-IP` value, then canonicalizes that
address. It ignores public forwarding headers on the direct TCP listener.
An embedded router without a listener identity has no peer. The staff server
keeps its 16-request admission limit and 10-second handler deadline and now
also bounds connections at 16 using the shared header and connection deadlines.
`STAFF_BIND` remains required and loopback-only when the Unix profile is used.

Install [the service drop-in](../deploy/staff-proxy.service.conf) for the staff
unit when enabling this profile. Create the separate `board-staff-edge` group
and give only the dedicated staff proxy worker access to it. Keep credential
files outside the runtime socket directory and out of that group's read
permissions. Configure the worker's actual numeric UID in `STAFF_PROXY_UID`
and use `/run/board-staff/staff.sock` for `STAFF_PROXY_SOCKET`.
[The Nginx location fragment](../deploy/nginx-staff-proxy-location.conf)
overwrites the internal address header from the socket peer and removes
forwarding headers. It belongs inside the separately configured staff HTTPS
server; certificates, staff access policy and worker identity still require
operator configuration.
When using the Unix listener, direct TCP staff health checks must follow the
staff proxy instead. Keep that health route inside the private staff server.

The ordinary text form requires a chosen deletion password of 8–128 bytes.
A bounded Argon2 worker derives its stored hash and checks an existing OP
password when needed for OP markup. Public password deletion works for these
posts. The cross-origin anonymous-session and activity lifecycle remains
unfinished; this chosen-password form does not establish parity with that
lifecycle. Ordinary staff attachments and broader privileged workflows also
remain unfinished. Badge posts retain their moderation removal path. Full
source behavior and frontend parity remain the completion requirement; no
hosted deployment is claimed.

The shared-transport checkpoint passed local qualification on Linux:

| Check | Result |
|---|---|
| Source predicate regeneration | 224 Robot9000 and four early authentication-bypass cases |
| Rust source predicate comparison | All 224 Robot9000 cases |
| Shared HTTP suite | 43 tests, including moved socket, capacity, deadline and cancellation cases |
| Public application | 97 unit, two paired-listener and ten startup tests |
| Staff HTTP suite | Six tests, including the actual Unix listener's accepted UID, wrong UID, missing and duplicate address cases |
| Staff unit and persisted posting suites | 12 unit and 18 PostgreSQL cases |
| Public Robot9000 workflow | Passed through real persisted posting and browser transport |
| Configuration suite | 22 cases |
| Strict workspace Clippy | Passed; all targets/features with warnings denied in 1 minute 17 seconds |
| Existing generic transport and proxy-denial assertions | Compared with the preceding head and retained |

The first compile exposed a missing test dependency after moving the transport
suite. The dependency and two unused imports were corrected. A subsequent
Clippy scope ended without a final result and reported a zero-length
incremental dependency graph. The passing rerun disabled incremental
compilation within the same verified 3 GiB build scope, with debug assertions
and optimization unchanged. The owned database still has 67 successful
migration files through version 76; this change adds no migration.

Complete local workspace qualification was not rerun. The preceding
`5741bef` response-capture checkpoint passed the complete hosted Linux,
Windows and media/operations jobs, monitoring and dependency advisories.
The published `bc5a1da` shared-transport checkpoint also passed all exact-head
hosted checks: [build/test](https://github.com/frankischilling/26chan/actions/runs/37121298888),
[monitoring](https://github.com/frankischilling/26chan/actions/runs/37121298881)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/37121298892).
The build includes complete Linux Rust/database/browser, Windows visual and
media/operations qualification. Those results are historical for the later
Options and identity-source continuation.

The Options and identity-source continuation passed all 66 staff tests,
including 15 library cases, 18 persisted posting cases, six HTTP cases and
three actual-binary startup cases. The source Options fixture regenerates
exactly and Rust matches all 2,048 cases. Staff and public keys produce the
same thread labels, private count context and Robot9000 fingerprints for IPv4,
mapped IPv4 and IPv6. The same verified country fixture maps all three forms.
Malformed keys, invalid paths and an oversized country file fail before
listener binding; errors omit the supplied key and path.

Public, reader, writer and observer configurations reject staff identity and
proxy sources, including non-Unicode values. The isolation comparison covers
32 rejected child configurations and eight clean controls. All 24 shared
configuration cases and ten actual public startup cases passed. The isolated
media runner also rejects the staff poster key through its existing credential
filter. The first isolation run failed on an assertion expecting the observer
to use the other roles' error wording; the assertion now checks the observer's
actual denial message.
Strict workspace Clippy passed across all targets and features with warnings
denied in 1 minute 23 seconds, using the verified 3 GiB build scope and
incremental compilation disabled.

The published `4b0f157` Options/configuration checkpoint adds no migration and
passed all exact-head hosted checks:
[build/test](https://github.com/frankischilling/26chan/actions/runs/37123912697),
[monitoring](https://github.com/frankischilling/26chan/actions/runs/37123912681)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/37123912730).
Those passes are historical for the ordinary posting continuation below.

## Ordinary posting proof and qualification

[Migration 77](../migrations/0077_ordinary_staff_posts.sql) adds an ordinary
branch to the existing private proof table. The authentication role issues
a proof binding the body, prepared Options and trip, selected board policy,
authorized limit, wordfilter context and 14 derived metadata fields. These
include the canonical peer, deletion hash and optional proof of the current
OP credential. The staff writer supplies exactly that context in its insert
transaction. Runtime roles cannot read or write the proof table directly.

The consumer rechecks current rank, board scope, revocation, session expiry,
inactivity and recent authentication. It acquires account and session locks
before locking the intent, then rechecks deadlines after that wait. A proof
expires after 15 seconds, consumes once and shares the existing 32-intent
capacity with badge proofs. Failed ordinary writes discard their unused proof;
expired proof cleanup remains bounded. The owner stays a role without login,
superuser, role creation, database creation or RLS bypass.

Scoped functions expose only OP membership, the latest OP reply time and one
active public OP's derived deletion hash to the staff writer. Staff and
authentication runtimes receive no bulk OP-peer or deletion-table grants.
An owner trigger records private ordinary-post state only after the exact
proof is consumed. Each insert resets the marker, including retained badge
and private discussion paths. Existing badge proofs keep their original
consumer through a private delegate.

The ordinary posting continuation passed 73 local staff tests, including
25 persisted posting cases. Seven new cases cover all four ranks, password
deletion, OP state, admission outcomes, Robot9000 rollback and bypass, proof
forgery, private grants, actual Unix posting and expiry behind row locks.
The metadata comparison rejects all 56 field mutations across the four ranks;
separate checks reject body changes, replay and 30 private-role operations.
Four consumer cases observe actual lock waits and cross the proof, session
or recent-authentication deadline before releasing the lock.

[The upgrade check](../scripts/test-ordinary-staff-post-migration.sh) applies
migration 77 through the actual migrator login to populated version-76 state.
Historical posts, boards, threads, deletion records, OP peers and existing
proof fields remain unchanged. A pre-upgrade badge proof still consumes once
with its original trip and badge. Forged ordinary markers create no private
records. Privacy policies and runtime private-table boundaries remain intact.
The isolated integration database has 68 successful migration files through
version 77; the preceding version-76 qualification database is unchanged.

Both local Chromium staff workflows passed. The new case covers all four ranks
through script-free OP posting, sage, board flags, public/staff ID continuity,
desktop/mobile headers, live updating, ID highlighting/counts, native filtering
and public password deletion. The existing WebAuthn, moderation, recovery and
logout workflow also passes. Context request inspection accepts absent or empty
referrers; separate Chromium wire checks require staff media requests to omit
Cookie and Referer. The spoiler request count is scoped to its fixture board.

Initial browser failures exposed a missing fixture board scope, deletion-record
cleanup order and a subject containing a source-denied term. One owned failed
fixture was removed after verifying its database identity and local paths.
The migration's first attempts exposed syntax and function-owner grant errors;
the corrected migration applies under the actual migrator login. Strict
workspace Clippy passes across all targets and features with warnings denied
in the verified 3 GiB scope, with incremental compilation disabled. Formatting,
documentation links and whitespace checks pass. Retries, browser deadlines and
screenshot baselines are unchanged. Failure logs remain local.

Complete local workspace qualification was not rerun for the ordinary posting
checkpoint. Published head `7a58929b2897fc02e4ac87ccfd51462110e0ddd3` passes
[build/test](https://github.com/frankischilling/26chan/actions/runs/37135101038),
[monitoring](https://github.com/frankischilling/26chan/actions/runs/37135101033)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/37135101036).
Build/test includes Linux application/browser, Windows visual, media/operations
and aggregate qualification. Later changes require their own exact-head checks.

Hosted qualification on `c54543f` failed in fresh role bootstrap before Linux
application or privileged qualification ran. Its old assertions required the
bounded content-admission and Robot9000 functions to be callable only by the
public runtime. Ordinary staff posting requires those functions too. The
bootstrap check now requires the exact public/staff grants while retaining
private schema/table denials and the function owners' restricted authority.
It also checks the new proof issuer, discard and scoped OP functions against
every runtime, denies all private policy/trigger/badge delegates, and bounds
the posting owner's new column reads and private-table writes. Fresh bootstrap
passes locally with all migrations; historical content remains unchanged.
Migration 77 is unchanged by this correction. The corrected published head
passes the complete hosted qualification linked above.

Robot9000 text-history retention is available through the separate
[staff cleanup interface](staff-robot9000-cleanup.md). Ordinary posting and its
explicit bypass keep the authority rules described above.
