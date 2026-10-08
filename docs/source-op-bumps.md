# OP self-bump intervals

[Issue #122](https://github.com/frankischilling/26chan/issues/122) implements
the supplied `imgboard.php:5975-6003` rule. An eligible reply from the OP's network
address sets sage when either the initial interval has not elapsed or the latest
surviving same-address reply is within the repeat interval. Both comparisons
are strict: equality permits bumping. The source selects that latest reply by
post number, not maximum timestamp, and counts replies that did not bump too.
Deleting the latest own reply can expose an older eligible one.

`config/global_config.ini:233-237` enables the rule with 900 initial seconds
and 300 repeat seconds. Initial overrides are 600 for `b`, `bant` and `v`,
300 for `soc`, and 120 for `s4s`. The corresponding board settings are
`op_bump_limit`, `op_bump_initial_seconds` and `op_bump_repeat_seconds`.
Operator-only integer policies accept zero through 2,147,483,647. Zero is an
interval of zero seconds, not a substitute for the separate enable switch.
Operators recreating a source board must configure its overrides explicitly.

Public posts use these intervals. Staff posts use the authenticated
`ordinary_timers` branch returned by the migration 0089 proof issuer: janitors
with a nonempty raw name or on a meta board receive the full initial and repeat
intervals, including private posting. The half-length janitor reply/image
cooldown does not discount OP self-bump intervals. Moderator-or-higher posts
and empty-name janitor posts on non-meta boards are exempt from self-bump
suppression. Display names, badges and client-supplied roles do not select this
branch. See [staff posting cooldowns](staff-posting-cooldowns.md).

Migration 0090 extends bump-only same-host evidence to private and badged posts
recorded in posting history. Exempt replies still enter that history, so a
later eligible same-host reply can use them. This does not expand public OP
markup, poster counts or their existing membership rules.

The server carries its captured request-start clock and transport peer into
the board-locked posting transaction. It compares canonical IPv4/IPv6 addresses;
IPv4-mapped IPv6 normalizes to IPv4. It does not trust forwarded headers,
cookies, names or deletion passwords for address matching. Shared NAT addresses
therefore share OP treatment, as in the source. This is a bump policy, not
authentication or proof that two posts have the same human author.

The rule supplies sage to the existing bump decision. Sticky/permasage still
prevents bumping; permaage overrides self-sage, ordinary sage, age and count
cutoffs. Existing closed/archive/reply/image admission remains unchanged.
Private membership, public insertion, counters and timestamps commit together.
No public `bumplimit` flag or JSON field is derived from the self-bump intervals.

## Private data and trust boundary

Migration 0024 adds `post_secrets.op_peers`, containing one address per active
thread, and `post_secrets.op_replies`, containing only the post/thread IDs of
matching-address replies. Other reply addresses are not retained by this rule.
The public database role can read and insert these records because posting
needs them. Staff, authentication, media and monitoring roles cannot read them.
Neither context nor private records have a public serializer or debug logger.
The application still avoids recording addresses in request/error logs.

This is sensitive data, not anonymized data. A compromised public process or
its database credentials can read active OP addresses and membership records.
These records grant no staff or media-processing authority. The staff role can
delete content without private-table privileges: fixed-search-path trigger
functions remove private data as part of the same transaction. Thread deletion
or archival removes the address and all memberships. Reply deletion removes
that reply's membership. Physical row deletion also cascades cleanup.
Rollback restores the private records along with the content state.

The source stores each post's host. Retaining only OP address and own-reply
membership reduces private data while supporting this rule; it does not supply
unique-visitor counts or a moderator address-search feature. Those features
remain separate. Deleted text retention follows existing policy, but these
private matching records are removed from the live database on the transitions
above. Backups can retain earlier records until their own retention expires.

## Bump-only posting history

Migration 0090 adds `content.posting_op_bump_context(bytea,text,bigint)`. It
matches the verified peer's private HMAC actor against the existing
[posting history](source-posting-cooldowns.md#private-identity-and-lifecycle),
including private `/j/` and badged posts. An OP history match proves ownership
for this policy. A reply match alone does not. The query selects the highest
surviving reply post ID and reads that post's `content.posts.created_at`, not
the maximum timestamp or the history row's request time.

Legacy ownership remains an independent fallback. Public posting uses the
existing address match and `op_replies`; staff posting uses the new
`content.staff_op_bump_context(text,bigint,text)` companion to the unchanged
staff OP context. The companion preserves known ownership even when there is
no prior reply, so the initial window still applies. Its legacy lookup remains
limited to public boards other than `/j/`.

The writer combines ownership from either source and chooses the highest
reply post ID across both sources. After key rotation, a surviving legacy OP
address can still establish ownership while a current-key reply supplies the
latest content timestamp. Neither source has to supply both facts. Without
legacy ownership, an OP disconnected by key rotation remains unknown even if
current-key replies exist. Missing historical private or badged identity cannot
be recovered by this lookup. Migration 0090 does not backfill history or claim
full historical host parity.

Both functions have fixed search paths and NOLOGIN, NOBYPASSRLS owners. Public
and staff roles can call the HMAC context, but it checks the actual invoker and
withholds private-board results from the public role. Only staff can call the
legacy companion at runtime. The migration adds no runtime history-table
reads or legacy-table grants. Existing deletion/archive cleanup removes the
corresponding evidence; undelete does not reconstruct missing history.

## Deployment and recovery

The original address records require migration 0024. For the current writer,
stop public and staff posting, apply migrations through 0091 and deploy matching
binaries before restoring posting. Migrations 0090 and 0091 reuse existing owners
and add no bootstrap role. See [archive deletion-password retirement](archive-deletion-secrets.md)
for the 0091 boundary. The upgrade does not invent addresses or HMAC history
for historical rows: old threads without either ownership source cannot apply
self-bump suppression. Imports need authorized source identity data if
historical self-bump behavior is required. Every application write requires a
configured posting key and a resolved peer, including direct fixture/import
calls. Direct tests supply explicit owned test keys and peers; they do not
qualify the production transport. Production posting requires trusted transport
identity and fails with 503 when it is absent, even if a request supplies
forwarding headers or a forged internal peer extension.

Linux public production requires the [verified Unix proxy](public-proxy.md).
Only a kernel-authenticated proxy UID may provide the canonical client address.
Direct development TCP still ignores forwarding headers. The owned HTTPS test
checks address-only own-reply records independently of password ownership;
qualification of the actual production edge remains required.

An older writer may lack the bump-evidence queries or required posting actor
context. Keep the additive schema during rollback and serve read-only until
a matching writer is available; resuming an old posting binary is not a safe
writable rollback. Backups need the same private
access controls and encryption as deletion secrets and staff data. The restore
exercise now seeds a synthetic OP address and own reply, compares both private
table fingerprints, and tests restored nonpublic-role read denials. It retains
the existing metadata, capability and grant checks. No production retention,
backup durability or recovery-time claim follows from disposable CI.

## Verification

Domain tests exercise strict initial/repeat boundaries, disabled policy, zero,
source values and extreme integer inputs. Public library tests verify that
production rejects missing transport context before database access.
The persisted regression checks different and matching peers, IPv4-mapped
normalization, same-host sage replies, post-number ordering, deletion, policy
transitions, flag precedence, late-insertion rollback, eight serialized replies
with a healthy blocked-writer witness, IPv6 HTTP requests, forged forwarding
headers, public-field absence, actual staff read denial and transactional
deletion/archive cleanup.

`scripts/test-op-bump-migration.sh` upgrades owned historical rows without
inventing identity, exercises defaults/bounds, real public private-row writes,
staff/media read denials and cleanup/rollback. These are coverage descriptions,
not current-head execution results.
[Posting timestamp assignments](source-posting-times.md) have separate
persisted and upgrade coverage.

`crates/store/tests/staff_op_bumps.rs` covers authenticated timer eligibility,
exempt-reply continuity, strict edges, post-number ordering, flag precedence
and rollback. Unit tests in `crates/store/src/op_bump.rs` cover highest-ID
merging, independent legacy ownership/current-key reply evidence, unknown
ownership and malformed candidates.

`crates/store/tests/private_op_bumps.rs` covers private/badged continuity,
authenticated eligibility, content-time selection, missing history, key
rotation, deletion/archive behavior, rollback and access boundaries.
`scripts/check-private-op-bump-migration.sh` covers the populated migration
0089-to-0090 upgrade. These are coverage references, not test-pass or
CI-completion claims for migration 0090. Missing historical private/badged identities,
key-disconnected history without a legacy fallback, remaining staff flood
privileges, other cooldown/admission policies and deployed production
qualification still limit parity.
