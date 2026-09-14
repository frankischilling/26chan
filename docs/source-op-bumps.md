# OP self-bump intervals

[Issue #122](https://github.com/frankischilling/26chan/issues/122) implements
the supplied `imgboard.php:5975-6003` rule. A reply from the OP's network address
sets sage when either the initial interval has not elapsed or the latest
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

## Deployment and recovery

Apply 0024 after 0023, before deploying the binary. The upgrade does not invent
addresses for historical rows: old threads without private identity cannot
apply self-bump suppression. Imports need authorized source identity data if
historical self-bump behavior is required. Internal fixture/import calls may
omit peer context explicitly. Development router tests without connection
metadata remain identity-unknown; real listeners provide their socket peer.
Production posting fails with 503 when transport identity is absent, even if a
request supplies forwarding headers or a forged internal peer extension.

Reverse proxies must preserve the actual client transport identity through a
separately qualified configuration. Passing untrusted X-Forwarded-For is not
supported. A proxy that appears as every client's socket peer groups all of
them together and is not source-equivalent deployment evidence. Existing
production proxy/TLS qualification remains unfinished.

Older binaries ignore the new intervals and do not populate new private rows.
Do not discard the additive schema during binary rollback; record the loss of
self-bump enforcement and identity continuity. Backups need the same private
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
staff/media read denials and cleanup/rollback. Local domain/public tests and
all-target Clippy pass. PostgreSQL is unavailable locally; persisted, upgrade
and populated restore execution require complete current-head Linux CI.
[Posting timestamp assignments](source-posting-times.md) have separate
persisted and upgrade coverage. Staff flood privileges, other cooldown/admission
policies, unique-visitor accounting and deployed production qualification
remain unfinished.
