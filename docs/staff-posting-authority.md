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

The Rust Robot9000 predicate now records that distinction. The current staff
writer still supports badge posts and private discussion; authenticated
unbadged posting remains unfinished. Its proof must bind the selected policy
and ordinary identity fields, recheck current authority, and preserve the
existing expiry, replay, revocation and board-scope checks.

`STAFF_POSTER_ID_KEY` and `STAFF_COUNTRY_DATABASE` load optional ordinary
identity sources under staff-specific names. Use the same private poster key
as the public application's `POSTER_ID_KEY` to preserve thread labels, private
counts and Robot9000 identity across both listeners. Use the same verified
country data under the staff-specific path. The key parser rejects malformed
and all-zero values. The country loader requires an absolute regular file,
limits it to 64 MiB and verifies the country database format before serving.
Startup errors disclose neither the key nor the supplied path. Inherited
public identity variables and unrelated database credentials remain rejected.
The current badge writer does not consume these ordinary sources; configuring
them alone does not enable unbadged posting.

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

This is preparation for ordinary staff IDs and flags. It does not enable
unbadged posting or establish a deployed staff proxy qualification. Full source
behavior and frontend parity remain the completion requirement.

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

This continuation adds no migration. Ordinary staff posting still requires a
single-use proof that binds its prepared Options, identity fields and body,
and a posting form that selects the ordinary policy. Those paths remain
unfinished. Complete local workspace qualification was not rerun, and the
continuation needs its own exact-head hosted checks after publication.
