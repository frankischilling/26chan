# Staff posting authority and peer transport

The pinned source separates authentication from the visible staff badge.
Robot9000 checks an ordinary post even when its author is authenticated. It
bypasses that check only when the raw Options field is exactly `bypass_r9k`
and the caller has janitor-or-higher authority. A selected staff badge also
bypasses Robot9000. Case changes, surrounding spaces and combined commands
do not match the ordinary-post bypass.

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

This is preparation for ordinary staff IDs and flags. It does not enable
unbadged posting or establish a deployed staff proxy qualification. Full source
behavior and frontend parity remain the completion requirement.

Local qualification passed on Linux:

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
Those results are historical for this transport change. Its exact-head hosted
checks remain required after publication.
