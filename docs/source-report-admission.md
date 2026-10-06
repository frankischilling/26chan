# Report admission and retained identity

Migration 0094 implements the IP branch of `4chan-old/modes/report.php:110-168`.
It does not establish complete report parity. Source password and authenticated
Pass matching, CAPTCHA, report bans, category weights and illegal-report policy
remain separate work.

## Decisions

An eligible target is checked in this order:

1. A surviving report by the same actor for the same board/post rejects a duplicate.
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
report actor. An anonymous capability is not the source's deletion password,
and changing addresses is not claimed to preserve password or Pass matching.
Missing server key or trusted peer fails closed. Historical reports receive no
invented identity; pre-migration reports and reports across key rotation are not
retroactively matched.

## Transaction and storage boundary

GET performs a read-only advisory check. POST always checks again in a Read
Committed transaction, taking the board lock before the singleton admission
lock. The decision clock is captured after waiting for those locks. Report and
private membership insertion happen in one restricted database function;
anonymous activity registration then occurs in the same transaction. Any later
failure rolls all three back. Public direct report insertion and sequence access
are revoked. Public and staff runtime roles cannot read private actor metadata.
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

Whole-post and whole-thread deletion retire the corresponding private
memberships, including thread rollover and archive expiration. Public file-only
deletion preserves membership. The staff file-removal action retires membership
only after a fresh successful file deletion; repeating the action cannot clear
reports submitted later. Existing attachment visibility limits still apply.

Retirement never takes the admission gate or anonymous-session locks. Raw
row-first lifecycle updates use a nonwaiting board lock under Read Committed,
so they cannot introduce a tuple-to-board wait cycle. Normal callers already
hold the board lock. A concurrent, uncommitted removal can conservatively keep a
report counted; it cannot allow excess admission. Rollback restores membership.

Only private admission membership is retired. Existing report history and audit
records remain. Ordinary resolution/dismissal does not release capacity.
Archive-only transitions preserve membership because the source's conditional
illegal-report count cannot be reconstructed from free-text reasons. Source hard
clear and reporter-wide purge are not equated with resolve/dismiss. These remain
explicit lifecycle gaps.

## Rollout and checks

For an existing installation, the bootstrap administrator runs
`deploy/report-admission-role.sql` once before migration 0094. Fresh installations
use `deploy/roles.sql`. Stop old writers, migrate, then start matching public and
staff binaries. Both readiness checks require the new restricted interfaces.
Old public binaries cannot insert reports after the direct grant is revoked;
rolling back only the binary is not a compatible reporting rollback.

The migration qualification script checks fresh and populated upgrades, retained
rows, private authority and current dump/restore. Store tests cover strict time
edges, concurrency, rollback, session cleanup and retirement. HTTP tests cover
trusted identity, duplicate rejection and response-budget failure. Exact run
outcomes belong to the pull-request checkpoint; listed checks are not evidence
that every platform or browser has passed.
