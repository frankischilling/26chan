# Staff posting cooldowns

The supplied `4chan-old/imgboard.php:6004-6016` selects the latest post by
post number for the same host and board, including OPs and replies. Although
its nearby comment says three seconds, the executed assignment is
`$cooldown = 5`. The comparison is strict: four elapsed whole seconds reject
with one second remaining; exactly five seconds allow the attempt. A newer
post number with an earlier request clock controls the decision, even when
an older post has a later timestamp.

Migration 0088 adds `content.check_staff_posting_cooldown(bytea,text,bigint)`
and an all-post `(board, actor_hash, post_id DESC)` history index. The check
uses the same private transport-derived actor and surviving history as
[ordinary posting](source-posting-cooldowns.md). It is board-local and covers both new threads and replies,
including attachment posts. It returns the source reply cooldown kind even
for an attempted OP. Ordinary board intervals do not change its five-second
value. Deletion and archive retain the [ordinary history lifecycle](source-posting-cooldowns.md#private-identity-and-lifecycle).

Authenticated janitor-or-higher posting uses this gate after successful staff
authority issuance, including unbadged staff posts and private `/j/` posts.
Badge display is not authorization. The staff proof issuer and proof-consuming
insert trigger still check session, role and board scope. Requests without
valid staff authority do not gain the five-second route merely by supplying
staff-looking fields. The actor lock, timer decision and content write use the
same posting transaction; rejected writes leave no post or successful action.

Migration 0092 adds the separate [IP-only active-thread quota](source-user-thread-quotas.md)
for every OP, including badged, unbadged and private `/j/` posts. Staff timer
exemptions do not bypass it. Source password/Pass matching remains unimplemented.

Only `board_staff` can execute the new decision function at runtime. Its
NOLOGIN owner and fixed search path match the ordinary timer functions.
Migration 0088 changes no board intervals or existing history rows and does
not backfill identity for historical posts. It also adds the auth-only
`staff_identity.discard_badged_post_authority(bytea,bytea)` cleanup function.
A failed badged/private post discards only the issued ticket matching that
session; ordinary proof rows use their existing separate cleanup API.

## Named and meta-board janitors

The source condition in `4chan-old/imgboard.php:5862-5866` depends on the
janitor's raw submitted name or the board's meta setting. Migration 0089 binds
three facts into each new private staff posting proof: `raw_name_nonempty`,
`is_janitor` and `meta_board`. The application captures whether the parsed raw
name is nonempty before display-name normalization. An empty raw name can
render as "Anonymous" without becoming named; a nonempty raw name can also
normalize to "Anonymous" without losing its named branch. Whitespace-only
input is nonempty if the parser preserves those bytes. Do not derive this
fact from a prepared display name, tripcode, capcode or user-provided role.

A janitor with a nonempty raw name, or any janitor on a meta board, enters the
ordinary timer branch before the separate five-second gate. Reply and image
intervals are `ceil(board_interval / 2)`: an interval of 17 seconds becomes
9 seconds, while 23 becomes 12. The incoming post's attachment state chooses
the interval. The newest surviving reply by post number supplies the history,
including replies in other threads on that board. Equality is allowed.

Same-board new-thread intervals remain full length. Cross-board new-thread
actions retain the inclusive five-minute rule and database clock. An empty-name
janitor on a non-meta board uses only the staff gate. Moderator-or-higher
accounts use the staff gate regardless of the displayed badge or name. This
is an authenticated-role decision, including ordinary unbadged and private
`/j/` posting; public posting cannot request the janitor discount.

## OP self-bumps

The writer also uses the migration 0089 issuer's authenticated `ordinary_timers`
result to gate [OP self-bump suppression](source-op-bumps.md). Public posts and
janitor posts with a nonempty raw name or on a meta board use the full initial
and repeat OP intervals, including private posting. These intervals are not
halved.
Moderator-or-higher posts and empty-name janitor posts on non-meta boards skip
self-bump suppression; their separate five-second staff gate still applies.

Migration 0090 reads the private posting HMAC history for bump-only ownership
and latest-reply evidence, extending same-host continuity to recorded private
and badged posts. Exempt replies remain usable by later eligible replies.
Legacy OP ownership is an independent fallback, including when no reply exists;
reply candidates from both sources are merged by highest post ID using the
selected content row's timestamp. Current-key reply evidence can therefore
combine with legacy ownership after key rotation.

Public OP markup, poster counts and their narrower membership rules stay
unchanged. So do explicit sage, permaage, sticky/permasage precedence, the
request-start clock and the authenticated `ordinary_timers` eligibility gate.
The [OP self-bump documentation](source-op-bumps.md#bump-only-posting-history)
describes the access boundary and missing-history limits. There is no backfill:
unknown historical private/badged identity and key-disconnected ownership
without a legacy fallback still prevent full host parity.

## Proof authority

The current limited, source and ordinary issuer overloads accept the raw-name
boolean and return the branch chosen from the locked, authenticated account
role and board policy. The writer uses that returned branch rather than a
role snapshot taken before authentication. Session expiry, recent
authentication, account revocation and board scope still apply.

The consumer checks the bound raw-name context, janitor classification, board
meta policy and existing exact post payload before consuming the single-use
proof. A role change crossing the janitor boundary, changed meta policy or
mismatched raw-name context rejects the proof. Existing consumer checks still
cover session, scope, policy and payload changes. The account/session locks
and rechecks remain part of proof consumption.

Only `board_auth` can execute the current issuers. Earlier issuer overloads,
including the original and wordfiltered issuers, remain available internally
to their owner but are no longer callable by runtime roles. Only `board_staff`
can call the proof consumer and janitor timer wrapper. The underlying consumer,
raw-name binder and discount-capable timer core remain private. Definer
functions retain their NOLOGIN owners and fixed `pg_catalog,pg_temp` search
paths; the timer owner gains no account, session or proof-table access.

The posting application supplies the peer-derived actor and parsed raw-name
fact. These checks constrain supported runtime entrypoints; they do not prove
an honest HTTP request against a compromised application database role.

## Upgrade and qualification

Stop public and staff writers, apply migrations through 0092, and deploy
matching binaries before restoring posting. Migrations 0089 through 0092 need no
new bootstrap role. Migration 0090 leaves existing content and history intact;
0091 adds [archive deletion-password retirement](archive-deletion-secrets.md).
Migration 0089 does not recover old posting identity or invent context for
existing proofs. Its three added fields remain
null on pre-upgrade proofs. Those proofs fail closed even if their original
15-second lifetime has not elapsed. Drain in-flight work and retry affected
requests through the new issuer; never backfill context from display names.
An old posting binary is not a writable rollback target.

`scripts/test-janitor-posting-cooldown-migration.sh` prepares a populated 0088
PostgreSQL 16 cluster, applies only 0089, and compares application/private rows
and unrelated function/RLS authority. It also exercises actual runtime logins,
retired issuer denial, private cores, authenticated return branches,
raw-name/meta binding, public discount denial, half reply/image edges and
unchanged OP/cross-board edges. The separate integration tests cover posting
transactions and changes to live authority. The presence of these tests is
not a test-pass or production qualification claim.

`crates/store/tests/staff_op_bumps.rs` covers the authenticated OP self-bump
gate; `crates/store/src/op_bump.rs` has unit coverage for merging HMAC and legacy
evidence. These sources make no new pass or CI-completion claim. Trusted Pass
discounts and duplicate-comment/image flood checks remain unfinished. Missing
historical private/badged identity and key-disconnected bump history still
limit parity; complete posting-admission parity remains unfinished.
This slice does not close [issue #213](https://github.com/frankischilling/26chan/issues/213).
