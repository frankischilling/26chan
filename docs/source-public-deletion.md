# Public deletion eligibility

The ordinary and legacy deletion routes apply the supplied source gates from
`imgboard.php:2464–2530`. They use the same rules for whole-post and file-only
requests. Staff and automatic maintenance retain their separate authority.

The source checks the board's OP/reply deletion switch and rejects posts at least
1,800 seconds old before checking ownership. This upper age uses the server's
request-start time. An authorized public request still cannot delete an archived
post, a sticky OP, a `/vg/` OP, or an OP with a surviving staff-badged reply.

Migration 0091 also [retires deletion-password hashes](archive-deletion-secrets.md)
for the OP and replies on new archive transitions. Historical archived hashes
remain unchanged pending a separate decision.

The minimum age is 60 seconds when the current anonymous session's network
lifetime is at least 900 seconds, and 600 seconds otherwise. Equality admits the
request at either minimum. The lower age uses the current server time after the
lock wait, matching the source's separate `time()` call. Verified level, overall
session age and the broader known-user predicate do not substitute for network
lifetime. A changed peer or environment can reset that lifetime.

Both public authority paths recheck policy and ownership while holding the board
mutation lock. Password recovery verifies the current stored hash fingerprint;
anonymous ownership verifies the current private membership. Session resumption
uses the server-resolved peer and environment. Public fields cannot supply a
request clock, trusted peer, ownership proof or known-user flag. Deletion does
not update session activity or refresh its cookie, matching the original caller.

## Configuration and migration

Apply migration 0084 before running the new binaries. It adds bounded operator
policy columns with source defaults of 60/600/1,800 seconds, preserves existing
posts and installs the active source `NO_DELETE_OP` overrides. The commented
`/qa/` reply setting is not enabled. The `/vg/` OP restriction remains a code-level
rule. Public and staff runtime roles receive no policy-write authority.

A binary rollback retains the additive schema but restores older, more permissive
public deletion behavior. It is not an equivalent authorization policy.

The development seed gives newly created `/fixture/` and `/demo/` boards explicit
zero minimum ages for unrelated test flows. Imported board policies stay strict.
Browser cleanup on imported boards requires an exact owned post receipt and a
per-test marker. The privileged test helper checks both before aging an owned
reply or removing its fixture thread; it never relaxes the board policy.

## Per-peer request quota

Migration 0085 adds the private cross-board counter. Configure `POSTER_ID_KEY`
as described in [deployment configuration](poster-ids.md#configuration-and-persistence).
It must remain stable across restarts and replicas. Both deletion endpoints
require that key and a verified current transport peer in every environment.
Missing either fails closed with 503; cookies, forwarded headers and form fields
cannot supply the quota identity. The authenticated Unix-proxy transport keeps
its existing verified-peer rules. The private keyed digest is never returned to
clients or logged.

At most three successful requests per hour and eleven per day are admitted,
including the exact hourly/daily boundary. A modern request has one server-owned
batch; one legacy submission shares one batch across its ordered selections.
Only the first successful mutation charges that batch. Failed authorization,
ineligible targets, no-op file requests and a single missing legacy target do
not charge it. Earlier committed selections remain deleted and charged if a
later selection fails. This intentionally closes the source's partial-failure
bypass: its action log was written only after the entire batch finished.
No client batch token is accepted.

A read-only quota precheck runs once before target eligibility, ownership or
legacy missing-target handling, preserving source error precedence without
charging a request. Reservation rechecks the current quota under its lock.
Reservation and the first mutation share a database transaction and roll back
together. Hourly/daily exhaustion returns the existing 403 source-flood response;
counter capacity or storage failures return 503. The bounded actor table has
opportunistic expiry cleanup rather than a guaranteed physical retention period.
It retains at most eleven events for each of 100,000 actors, removes at most 64
expired actors per new admission, and never evicts live history. New actors hold
a shared capacity guard through their first mutation transaction; existing actors
need only their own row lock. This can serialize unrelated first-time requests
under contention.

## Verification

The domain tests exercise minimum, maximum and network-lifetime boundaries and
protection/error ordering. Store tests cover both ownership paths, whole/file-only
rejections, unchanged state, resumed session fingerprints, policy changes and
revocation during lock waits, imported policy and denied runtime writes.

Public tests cover the ordinary route and both legacy encodings, password
rotation/reassignment, changed real transport peers and forged forwarded headers.
The separate fixture-helper test checks wrong boards, IDs and markers, positive
owned operations, and an unchanged board policy. Browser scenarios retain real
public deletion where it is allowed and explicitly reject archived deletion.

These checks do not qualify production authentication or every browser platform.
Exact commands and outcomes belong to the corresponding pull-request checkpoint.
[Quota fixture isolation](deletion-quota-verification.md) explains the separate
limits of happy-path browser qualification.

## Remaining scope

This slice does not complete issue #214. [Legacy batches](legacy-actions.md#multiple-selections)
now preserve submitted order and partial success. The source's peer-address-only authority,
physical file erasure and historical cleanup remain separate work.
[Fresh whole-deletion report cleanup](deleted-report-cleanup.md) removes target
report rows transactionally while preserving independent staff audit records.
[Fresh content erasure](fresh-content-erasure.md) clears payload and per-post author
authority while retaining structural media tombstones. The rewrite continues using
opaque session capabilities and hashed recovery passwords for live posts.
Shared-network peer equality alone does not grant deletion authority.
