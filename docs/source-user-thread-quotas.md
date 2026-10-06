# Active thread quota: IP-only matching

Migration 0092 adds the IP branch of the supplied per-user active-thread
quota. `4chan-old/imgboard.php:5691-5697,9537-9571` checks every new thread,
including authenticated staff and private-board posts. Replies do not consume
the quota. There is no capcode, role or private-board exemption.

This slice matches only trusted transport-derived IP history. The source also
matches existing UserPwd/password or Pass identity with `OR`; those branches
remain unimplemented. This is not full per-user or posting-admission parity
under [issue #213](https://github.com/frankischilling/26chan/issues/213).
The source was read as text, not executed.

## Count and board policy

The source counts same-board OPs that are not archived and whose stored
timestamp is strictly greater than `request_time - period_hours * 3600`.
The rewrite counts surviving, nondeleted OPs in nondeleted, unarchived threads,
with matching private IP history. It reads `content.posts.created_at`, not
history registration time, and uses the captured whole-second
[request-start clock](source-posting-times.md). An OP exactly at the cutoff
does not count. There is no upper timestamp bound, so a future-dated surviving
OP can count. Sticky, closed and saged threads still count if otherwise eligible.

Admission rejects when the existing count is greater than or equal to the
maximum, before inserting the proposed OP. A zero maximum rejects every new
thread; it does not disable the rule. A zero period still uses the strict
timestamp comparison. Replies and OPs on other boards do not consume this
board's quota. This policy is separate from the total board thread limit,
rollover and [posting cooldowns](source-posting-cooldowns.md).

`content.boards.user_thread_limit` defaults to 5 and
`user_thread_period_hours` defaults to 24, matching
`4chan-old/config/global_config.ini:193-195`. Migration 0092 applies the active
board configuration values, ignoring commented examples:

- Maximum 3: `/a/`, `/bant/`, `/i/`, `/pol/`, `/qa/`, `/v/`, `/vm/`,
  `/vmg/`, `/vrpg/`, `/vst/`
- Maximum 50: `/test/`
- Period overrides: `/i/` 168 hours, `/pol/` 6, `/qa/` 48, `/qst/` 72,
  `/news/` 120

Other values retain the global defaults; `/qst/` and `/news/` explicitly use
maximum 5 in the source. New synthetic boards receive the column defaults.
The rewrite bounds the maximum to 0..100000 and the period to 0..876000 hours.
These bounds are operational limits, not source constants. Policy changes
serialize with posting through the board-row lock.

## Identity and lifecycle

The quota reuses the full private HMAC actor from
[posting history](source-posting-cooldowns.md#private-identity-and-lifecycle).
The trusted transport peer supplies the IP, independent of cookies, sessions,
names, submitted passwords, displayed poster IDs or client-supplied fields.
IPv4-mapped IPv6 addresses normalize to IPv4; other IPv6 addresses retain their
full address. Shared network addresses share this quota. It is not proof that
posts came from the same person.

OP or thread deletion and archive remove the applicable history; rollback
restores it with the content. File-only deletion retains it. Undelete does
not reconstruct removed identity. There is no backfill or new identity table:
unknown historical IPs remain unknown, and key rotation disconnects old
history from new requests. Legacy OP-bump address evidence is not a fallback
for this quota. These limits and the missing password/Pass branches can cause
the rewrite to count fewer OPs than the source.

## Transaction and authority boundaries

`content.check_user_thread_quota(bytea,text,bigint)` returns only the rejection
decision and board policy. It does not return a count, digest, IP address or
matching post IDs. Runtime roles gain no access to private history. The
existing `board_posting_cooldown_owner` remains NOLOGIN and NOBYPASSRLS; the
security-definer function has a fixed `pg_catalog,pg_temp` search path. Only
`board_public` and `board_staff` receive runtime execute permission. The owner
receives narrowly scoped reads of the two new policy columns and reuses its
existing content/history access. Public callers cannot use the function for
private boards.

The writer uses Read Committed isolation and the existing actor-stripe, global
OP-capacity and board lock order. The decision and insert share that
transaction. Public OPs check the quota before content filters, ordinary
cooldowns and rollover. Staff OPs check it after full authority issuance bound
to the filtered body, before staff/janitor cooldowns and rollover. All authorized
staff branches use it, including
badged, unbadged and private `/j/` posts; the
[staff timer exemptions](staff-posting-cooldowns.md) do not exempt this quota.

Staff error and effect ordering still differs from the source, which checks the
quota before filters for all OPs. Ordinary staff and badged `/test/` attempts
can commit a content-filter rejection or quiet effect before reaching the
quota. There is no existing safe preflight API that resolves this gap without
authentication changes. Every successful staff OP still requires full authority
and the quota check before cooldowns and rollover.

A quota-rejected attempt rolls back content, attachment consumption, private
history, OP actions and rollover changes. Failed staff posting also discards its unused
issued proof through the existing cleanup path. A quota check grants no staff
authority and does not replace the independent admission or proof checks.

The source `S_TOOMANYTHREADS` wording is:
`Error: You may not post more than N active threads at a time.`
It uses singular `thread` when N is 0 or 1. The application returns this through
its existing content-rejection response paths. No private quota history is
added to responses, logs or public serializers.

The source returns success when its SQL query fails. The rewrite deliberately
fails closed on SQL failures, missing or invalid decision results, or unavailable
identity. Infrastructure errors remain service failures rather than fabricated
quota rejections.

## Deployment and verification

Stop public and staff writers, apply migrations through
`0092_user_thread_quota.sql`, and deploy matching binaries before restoring
posting. Migration 0092 reuses the existing owner and requires no new bootstrap
role. Preserve the existing [key and proxy setup](source-posting-cooldowns.md#deployment).
An older writer can bypass this new admission check, so it is not a safe
writable rollback target. Read-only serving with the migrated schema is a
separate option.

Public and staff readiness inspect the decision function's signature,
security-definer owner, fixed search path and restricted execute grants, plus
the required policy columns and read grants. These are metadata checks; they
do not probe private actors or history.

The migration and implementation are not evidence of a passing test run or
production qualification. This document makes no new test-pass or CI claim.
Password/Pass matching, unknown historical identity, key-disconnected IP
history and staff filter/quota ordering remain explicit gaps.
