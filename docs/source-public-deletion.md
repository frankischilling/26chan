# Public deletion eligibility

The ordinary and legacy deletion routes apply the supplied source gates from
`imgboard.php:2464–2530`. They use the same rules for whole-post and file-only
requests. Staff and automatic maintenance retain their separate authority.

The source checks the board's OP/reply deletion switch and rejects posts at least
1,800 seconds old before checking ownership. This upper age uses the server's
request-start time. An authorized public request still cannot delete an archived
post, a sticky OP, a `/vg/` OP, or an OP with a surviving staff-badged reply.

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

## Remaining scope

This slice does not complete issue #214. The source's peer-address-only authority,
multiple selected posts, hourly/daily action counters, physical row/file erasure
and report cleanup remain separate work. The rewrite continues using opaque
session capabilities, hashed recovery passwords and its existing retention model.
Shared-network peer equality alone does not grant deletion authority.
