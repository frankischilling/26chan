# Posting cooldowns

Migration 0087 adds server-side ordinary posting timers from the supplied
source. Migration 0088 adds the five-second authenticated staff gate; migration
0089 adds proof-bound named/meta janitor timers. Migration 0090 reuses the
private history for [OP self-bump evidence](source-op-bumps.md#bump-only-posting-history). This is one part of
[issue #213](https://github.com/frankischilling/26chan/issues/213),
not complete posting-admission or source parity. The source references below
were read as text; the original PHP was not executed.

Migration 0092 also reuses that history for the
[IP-only active-thread quota](source-user-thread-quotas.md). It applies to all
OPs independently of cooldown exemptions; password/Pass matching remains open.

## Source rules and board policy

`4chan-old/imgboard.php:5887-5900` checks same-board and cross-board new
threads. Lines 5933-5960 check ordinary replies. The reply check uses the
latest surviving reply by post number, not the greatest timestamp, across
threads on the same board. The current submission selects the text or image
interval; the previous reply need not have the same attachment type. OPs do
not enter that reply history.

The same-board OP check uses the greatest surviving OP request timestamp.
The source predicate is `root > 0`; the rewrite retains the identity of a
surviving OP when its sticky status changes. Its separate sticky state and
root representation are not a claim of full source-format equivalence.

Board columns `posting_reply_seconds`, `posting_image_seconds` and
`posting_thread_seconds` hold intervals in seconds, each bounded to
0..86400. Migration 0087 applies the global/category defaults and the named
board overrides in `4chan-old/config/`:

| Board policy | Text reply | Image reply | New thread |
|---|---:|---:|---:|
| Default / worksafe | 60 | 60 | 600 |
| Non-worksafe default | 60 | 30 | 600 |
| `/b/`, `/pol/` | 30 | 30 | 90 |
| `/bant/` | 15 | 15 | 60 |
| `/vg/` | 90 | 120 | 600 |
| `/jp/`, `/vt/` | 60 | 60 | 3600 |
| `/s4s/` | 60 | 30 | 300 |
| `/test/` | 60 | 60 | 30 |

New synthetic boards receive the column defaults. Operator changes to board
policy serialize with posting through the board-row lock. The transport's
separate per-peer request limit still applies; these persisted timers do not
replace it.

Same-board comparisons are strict: reject when the selected history time is
greater than `request_time - interval`. Equality is allowed. Both saved
history and the comparison use the server-owned whole-second request clock
from [posting timestamps](source-posting-times.md). An older request can
commit later, so reply post-number ordering and OP maximum-time ordering
must remain distinct.

The cross-board OP rule is fixed at five minutes. It rejects a successful
OP action on another board whose request timestamp is greater than or equal
to database time minus 300 seconds. Database time is sampled after acquiring
contended locks. The lower edge is inclusive: an action exactly 300 seconds
old still blocks; it clears at the next whole second. Successful OP actions
retain the greatest request timestamp for each actor/board pair.

## Native Quick Reply advisory

The server renders the current ordinary reply and image intervals as
`data-posting-reply-seconds` and `data-posting-image-seconds` on board/thread
posting forms and approved-upload forms. Native Quick Reply uses those values
with a per-board local success timestamp, `4chan-cd-<board>`, written only
after a confirmed Quick Reply success. The timestamp uses the client's
millisecond receipt clock. It neither exposes nor replaces the server's
private actor history, whole-second request clock or transaction decision.

The incoming draft selects text or image timing, including selected/pending
media and copied approved capabilities. The button displays remaining seconds
rounded upward. During a countdown, a valid ready draft's submit click toggles
one-shot automatic posting; expiry clears that intent before attempting the
post. Shift-click bypasses only the advisory. Server errors remain authoritative
and never trigger an automatic retry or record a local success.

Edits, replacement drafts, target changes and media changes fence the armed
intent. Close, feature disable and page suspension stop pending callbacks;
thread closure and upload readiness still prevent submission. A later upload
approval cannot revive canceled intent. Matching localStorage events refresh
the same-board countdown, but tabs retain independent automatic intent rather
than a cross-tab posting lock. Invalid/future timestamps and unavailable
storage do not block posting. The
[Quick Reply lifecycle](native-quick-reply.md#ordinary-reply-countdown-and-automatic-posting)
describes these client checks in detail.

This advisory covers ordinary replies only. It does not reproduce trusted Pass
discounts, staff-specific timers, duplicate flood rules or new-thread client
timing. It is not complete parity under the
[active reference boundary](compatibility.md#active-reference-boundary).

## Private identity and lifecycle

The application derives a full 32-byte HMAC-SHA256 digest from the verified
transport peer and the deployment poster key. IPv4-mapped IPv6 addresses are
canonicalized; other IPv6 addresses retain all address bytes. This digest is
domain-separated from public poster IDs, poster counts, deletion quotas and
Robot9000 fingerprints. It is private, cross-board and independent of cookies,
anonymous sessions, name, password, displayed ID or client-supplied fields.
Neither the digest nor the address belongs in responses or logs.

Two private tables serve different purposes:

- `post_secrets.posting_history` connects newly inserted posts to their actor
  and request time. Post deletion removes that post's history; thread deletion
  or archive removes the thread's history. Hard deletion also cascades through
  the post/thread foreign keys. File-only deletion retains posting history.
  Undelete does not reconstruct it.
- `post_secrets.posting_thread_actions` records successful OP actions. It has
  no post or thread foreign key, so content deletion and archive do not clear
  the cross-board timer. Deleting an entire board cascades its action rows.
  The content cascade also removes that board's post history.

There is no historical backfill: existing content has no recoverable private
posting identity. Historical imports as `board_migrator` without actor context skip registration.
An operator can supply an explicit synthetic actor for an owned fixture; that
insert takes the same validation and capacity path. Neither case recovers an
unknown historical identity. Keep these tables private in backups as well as
in the serving database; a dump can retain history already removed from live
content.

## Transaction and authority boundaries

The writer uses Read Committed isolation and locks an actor stripe, then the
OP action-capacity row when applicable, then the board and thread. The
ordinary decision and the post write share the transaction. Rejected posting,
including a Robot9000 rejection, does not leave a post, posting-history row,
OP action, consumed attachment or rollover side effect behind. Independent
Robot9000 mute behavior remains in force.

History registration is a private `AFTER INSERT` trigger on `content.posts`.
It derives the post, board, thread and request time from the inserted row and
reads a transaction-local actor context set by the application. There is no
runtime-callable function for attaching history to an existing or restored
post. The trigger checks the runtime role, private-board boundary, live thread,
actor encoding and isolation level. Its gate rechecks use `NOWAIT`, so a
writer that skipped the lock order fails instead of waiting in reverse order.

The dedicated `board_posting_cooldown_owner` is NOLOGIN and NOBYPASSRLS, with
narrow grants and a fixed `pg_catalog,pg_temp` function search path. Runtime
roles may call the lock and decision functions, but cannot read/write the
private tables or execute the registration/lifecycle trigger functions
directly. Existing RLS and staff insert-authority checks remain separate.

The actor context is application-supplied. This boundary prevents runtime
history attachment to arbitrary old posts; it does not cryptographically
prove a peer to a compromised application database role. Do not describe it
as protection against a compromised application inventing new content or
actor context.

## Authenticated staff timers

The five-second staff gate uses the newest surviving post number on the same
board, including OPs and replies. Named janitors and janitors on meta boards
also enter ordinary admission, with reply/image intervals rounded up to half
the board interval. Same-board OP and cross-board OP timers remain unchanged.
The issuer's authenticated account role selects this branch; a displayed
badge or a caller-supplied role does not. See [staff posting cooldowns](staff-posting-cooldowns.md)
for raw-name binding, role changes, proof lifetime and deployment requirements.

## Capacity and failure behavior

Storage is bounded without discarding live same-board history:

- 4096 fixed actor-lock stripes. A stripe collision only adds contention;
  comparisons still use the complete actor digest.
- At most 100,000 posting-history rows per board, with no TTL. Keeping old
  surviving rows preserves later policy increases and reversed request-time
  order. A full board history rejects new posts rather than evicting identity.
- At most 100,000 OP actor/board action rows globally. A new pair removes at
  most 64 expired rows, using the strict expiration edge `time < db_now - 300`.
  Updating an existing pair does not need a new slot. Remaining capacity
  exhaustion rejects the post.

Missing gates, unavailable storage, invalid decision results and exhausted
capacity fail closed. These caps and serialized admission are operational
choices in the rewrite, not source constants. Ordinary timer rejections use
the source reply/image/new-thread error wording; infrastructure failures are
service failures rather than fabricated cooldown decisions. Review
content retention and capacity before deployment. Do not clear live history
as a routine way to restore capacity, since that changes admission behavior.

## Deployment

For an existing installation, a bootstrap administrator must apply
`deploy/posting-cooldown-role.sql` before migration 0087. Fresh
`deploy/roles.sql` already creates the owner. Stop public and staff writers,
apply migrations through 0092 with the migration identity, and deploy matching
binaries. Migrations 0088 through 0092 reuse existing owners and require no
additional bootstrap role. Migration 0091 adds
[archive deletion-password retirement](archive-deletion-secrets.md).
Existing 15-second in-flight proofs must
be retried through the new issuer; migration 0089 leaves their new context
fields null and fails closed rather than guessing raw-name or role facts.
The new insert trigger requires actor context from those binaries; an older
writer is not a safe writable rollback target. Retaining the schema with
read-only serving is different from resuming an old posting binary.

Every posting path, including boards without displayed IDs and authenticated
staff posting, requires a configured private key and a resolved transport
peer. Use the same stable 64-hex-digit key material for public `POSTER_ID_KEY`
and staff `STAFF_POSTER_ID_KEY`, across replicas and restarts. Keep the variables
separate in each service environment and generate the material independently
of the tripcode key. Key rotation changes the actor digest, disconnecting new
requests from old private history; it is not transparent continuity.

Both services' `/readyz` require the key and check the migrated posting schema
and restricted insert trigger. Public readiness also checks the ordinary decision
function; staff readiness checks the staff and janitor decision functions and
current proof interfaces. Readiness does not establish that a request has a valid
posting peer; posting checks that separately. Key parsing remains optional
for read-only startup, but missing identity must never become a random key,
synthetic peer or permissive posting fallback. Configure the verified public
and staff proxy transports described in [public proxy](public-proxy.md) and
[staff posting authority](staff-posting-authority.md).

## Fixtures and remaining work

Owned tests supply synthetic deployment keys and transport peers through the
production posting path. Multi-board fixtures share a key where actor
continuity matters. Independent test groups may remove only their owned
posting history/action state using the migration identity. They do not
change runtime policy or grant production identities a reset API.

The browser fixture's `reset-posting` command requires its exact current
ownership lease, synthetic actor, database and unexpired marker. It removes
that actor's posting rows, leaves unrelated actors intact and does not reset
deletions, anonymous sessions or Robot9000 state. The deletion fixture's
separate reset continues to test deletion quotas. These scoped resets make
independent fixture scenarios possible without disabling production checks.

Test sources include `crates/store/tests/posting_cooldown.rs`,
`apps/public/tests/content_admission.rs`, and
`scripts/test-posting-cooldown-migration.sh`. The populated 0087-to-0088
qualification is `scripts/test-staff-posting-cooldown-migration.sh`. It uses an
owned disposable PostgreSQL 16 cluster and actual runtime logins. Its checks
cover OP/reply history preservation, unchanged application rows and existing
function/RLS authority, newest-ID ordering versus maximum time, strict four-
and five-second boundaries, OP-only history, private-board behavior, malformed
inputs, restricted execution and exact ticket/session cleanup. The existing
Linux privileged CI job runs it beside the ordinary cooldown migration test;
no CI platform or timeout change is required. The populated 0088-to-0089
qualification is `scripts/test-janitor-posting-cooldown-migration.sh`; its
checks cover private-state preservation, unbound old proofs, authenticated
issuer branches, runtime restrictions and half-reply/image versus unchanged OP
timers. These are qualification sources, not execution results.

The presence of these tests is not an execution result. This document makes
no test-pass or production qualification claim for the server admission fixtures.

For the current client advisory, nine Node helper cases and one Rust form-policy
render test passed; the Quick Reply bundle measures 27,349 bytes. The 22 new browser scenarios have not been run. Their synthetic browser
intervals do not alter database policy or qualify authoritative cooldown
behavior. See [Quick Reply verification](native-quick-reply.md#verification-and-remaining-work).

Duplicate-comment and duplicate-image flood rules, trusted Pass discounts,
new-thread client timing, missing historical or key-disconnected OP bump
evidence, and complete source-formatting equivalence remain unfinished. The ordinary native Quick
Reply countdown/one-shot implementation still needs its new browser scenarios
executed. This slice does not close #213 or establish complete
posting-admission parity.
