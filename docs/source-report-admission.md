# Report admission and retained identity

Migration 0094 introduced the IP branch of `4chan-old/modes/report.php:110-168`.
Migration 0097 adds forward registered automatic-session equality. This remains
partial report parity: verified-email and Pass identity, CAPTCHA, report bans,
effective category weights and illegal-report moderation policy are separate work.
Migration 0099 adds [opt-in category selection](source-report-categories.md)
without claiming those remaining abuse-control semantics.

## Decisions

An eligible target is checked in this order:

1. A surviving report matching IP or automatic identity for the same board/post rejects a duplicate.
2. Any matching report newer than 15 seconds rejects another report.
3. Thirty matching reports newer than one hour reject another report.
4. Eighty matching reports newer than 24 hours reject another report.

Windows span all boards and targets. Their lower edges are strict; a timestamp
exactly on the cutoff is excluded. There is no upper timestamp filter and no
calendar-hour or calendar-day reset. Resolved and dismissed reports still count.
Old surviving reports keep duplicate protection without an expiration date.
The configured source soft daily value has no executable check in the supplied
source and is not treated as another limit.

The actor is a keyed, domain-separated digest of the full trusted transport IP.
IPv4-mapped IPv6 is canonicalized to IPv4; distinct full IPv6 addresses remain
distinct. Cookies, submitted fields and untrusted forwarding headers cannot
choose the actor. The same address with a new anonymous cookie remains the same
report actor. A valid registered capability also resolves its private automatic
identity, so changing addresses does not evade that identity's reports. The
query uses one OR predicate and counts each row once even if both branches match.
Unlike OP quotas, report matching has no new-session or idle-reset exemption.
The capability is not treated as a literal source password or Pass credential.
Missing server key or trusted peer fails closed. Historical reports receive no
invented identity; pre-migration reports and reports across key rotation are not
retroactively matched.

## Transaction and storage boundary

GET performs a read-only advisory check using an existing verified cookie when
available. It never mints a capability, advances activity or refreshes a cookie.
POST checks target policy before session resolution, then checks authoritatively
in a Read Committed transaction: board lock, singleton admission lock, existing
session lock. The decision clock is captured after the session wait, and target
visibility, including archive expiry, is rechecked at that time. One restricted
database function inserts the report and membership and registers anonymous
activity. It leaves membership identity NULL until registration stamps the
same-transaction row. Any later failure rolls all changes back. Public direct
report insertion, sequence access and the old session-omitting mutation API are
revoked. The legacy IP-only mutation remains staff-only. Public and staff runtime
roles cannot read private actor metadata.
The success response is still reserved before mutation and emitted after commit.

Private membership survives anonymous-session cleanup. A dedicated NOLOGIN,
NOBYPASSRLS owner holds the table and serialization gate. Its functions use fixed
search paths and restricted grants; the runtime cannot invoke arbitrary cleanup.

The operator's membership limit defaults to 100,000 and accepts 1 through
1,000,000. Exhaustion fails closed after the source duplicate/time checks. It
never evicts old identities to admit new reports. This limits private membership,
not the total retained `content.reports` table. It is a defensive storage policy,
not a limit claimed to exist in the original source. Quota-storage failures also
fail closed rather than reproducing the source's query-failure bypass.

## Retirement

Migration 0122 removes the corresponding `content.reports` rows on fresh
whole-post and whole-thread deletion, including thread rollover and archive
expiration. Foreign-key cascades retire private membership, anonymous report
activity and captured weight evidence; empty groups retire through the existing
membership trigger. Public file-only
deletion preserves membership. The staff file-removal action retires membership
only after a fresh successful file deletion; repeating the action cannot clear
reports submitted later. Existing attachment visibility limits still apply.

Retirement never takes the admission gate or anonymous-session locks. Raw
row-first lifecycle updates use a nonwaiting board lock under Read Committed,
so they cannot introduce a tuple-to-board wait cycle. Normal callers already
hold the board lock. A concurrent, uncommitted removal can conservatively keep a
report counted; it cannot allow excess admission. Rollback restores membership.

Whole deletion now removes report history for that target. Independent staff
audit records remain. Historical reports on targets deleted before migration
0122 are not swept; the staff queue and report-resolution lookup exclude those
deleted targets. Their physical erasure remains separate operator work.
Ordinary resolution/dismissal does not release capacity.
Migration 0100 adds [forward archive retirement](report-group-lifetimes.md) for
complete categorical group lifetimes below the source's three-illegal-report
threshold. Missing or incomplete historical counters preserve membership.
Source hard clear is not equated with resolve/dismiss. Migration 0106 adds
[bounded reporter-wide clearing](reporter-clear.md) using captured IP and
automatic-session equality, with global moderator scope and atomic audit. It
retires matching memberships and hides marked rows from the active staff queue
while retaining report evidence. Legacy credential/Pass equality, direct-IP
input and the full source reporter interface remain separate gaps.

## Rollout and checks

For an existing installation, the bootstrap administrator runs
`deploy/report-admission-role.sql` once before migration 0094. Fresh installations
use `deploy/roles.sql`. Stop old writers, apply through 0122, then start matching
public and staff binaries. Both readiness checks require the new restricted interfaces.
The cleanup migration does not restore deleted reports on binary rollback.
See [whole-deletion cleanup](deleted-report-cleanup.md) for scope and checks.
Old public binaries cannot insert reports after the direct grant is revoked;
rolling back only the binary is not a compatible reporting rollback.

The migration qualification script checks fresh and populated upgrades, retained
rows, private authority and current dump/restore. Store tests cover strict time
edges, concurrency, rollback, session cleanup and retirement. HTTP tests cover
trusted identity, duplicate rejection and response-budget failure. Exact run
outcomes belong to the pull-request checkpoint; listed checks are not evidence
that every platform or browser has passed.

[Forward automatic-session equality](automatic-admission-identity.md) is captured
from migration 0095 onward and enforced for reports by 0097. Historical NULL
identities stay unknown; no category, email or Pass authority is inferred.
