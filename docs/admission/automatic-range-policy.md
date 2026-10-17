# Automatic mobile-range policy

Issue #212 includes automatic mobile-range events created by moderation. The
domain evaluator and event planner are implemented and tested. Migration 0180
adds private accepted-post comparison metadata. Migration 0181 adds a bounded
event store and posting enforcement, currently under qualification. The
template-qualified moderation producer and ban-request workflow are unfinished.

## Decisions covered

The automatic stage follows configured network rules. It applies to mobile
browser IDs on the exact board and submitted parent: zero for an OP, or the
thread ID for a reply. Reports have no parent and skip this stage. A matching
event must be younger than two hours, with a strict boundary. Database event
time, request-start time and anonymous activity time are separate inputs.

Verified or sufficiently established anonymous activity exempts the request
using the existing 120-minute, level-one source predicate. An actor with prior
posts and a network age of at least fifteen minutes only matches events created
at or before that network timestamp. Pending activity uses the same source
post-count rules as other admission stages.

The creation planner accepts a previously authorized, non-warning ban or ban
request using a template whose rule contains `global`. The source checks this
template predicate separately from the submitted local/global ban scope.
A direct ban records a range event unless a matching one is
younger than one hour. A first ban request records a hint. A later request with
a matching hint may record a range event, including when both requests concern
the same address. Hint retention includes the exact one-hour boundary; range
deduplication excludes it. Board, parent, browser and IPv4 /16 must all match.

Both input collections are bounded to 128 events. Invalid scopes, repeated IDs
and malformed mobile IDs reject the policy. Missing providers fail closed only
when needed by a candidate. Native IPv6 has no IPv4 prefix; mapped IPv4 uses
the canonical IPv4 prefix.

`PosterIdKey::automatic_range_prefix` derives a private HMAC-SHA256 comparison
token from the first two IPv4 octets. Its domain label differs from posting,
reporting and deletion identities. The token has no public formatter or
serialization implementation. Key rotation changes it, so old events cannot
be assumed to match after rotation. No raw address is required in an event.

## Source checks

Two extractors select bounded functions from hash-pinned source files:

    php scripts/extract-automatic-network-reference.php SOURCE fixtures/automatic-network-reference.json --check
    php scripts/extract-automatic-range-creation.php SOURCE fixtures/automatic-range-creation-reference.json --check

The admission fixture has 1,186 cases; the creation fixture has 96. They cover
clock boundaries, activity exemptions, pending activity, exact scope, first
hints and duplicate events. SQL callbacks model the selected source queries;
these checks do not establish database isolation or runtime caller parity.
The extractors do not load the source application, deployment configuration,
cookie codec or embedded browser-key function. Inputs are synthetic.

The current local domain suite passes 308 tests, including both fixtures and
the keyed-prefix checks. Strict domain Clippy passes. These results qualify the
domain code only.

## Accepted-post metadata

Accepted mobile IPv4 posts retain a server-derived browser comparison value
and keyed /16 token in private posting history. Capture includes badged staff
posts, independently of the filter's narrower enforcement scope. Unknown
browser metadata, desktop browsers and native IPv6 retain no pair. Old rows
remain NULL; the migration infers no historical evidence.

The insert trigger consumes a bounded request-local context bound to the
new row's board, parent and posting actor. It shares posting history's existing
application-role trust boundary. Headers influence the browser comparison
value; neither that value nor the prefix grants posting or staff authority.
Runtime roles cannot read or write the private columns directly.

Source thread moves preserve browser metadata and change reply scope to the
destination thread. Source-scope events remain on their original board and
parent. The separate source copy operation drops browser metadata.
The Rust move copies and verifies both private fields, includes them in its
prepared fingerprint, and rejects a later metadata change. Deletion follows
the existing posting-history lifecycle. Transfer and restriction seals cover
the changed schema and functions.

Local database qualification passes 56 tests across content admission,
randomizers, staff posting, cooldowns and thread transfer. Ten malformed or
mismatched contexts are rejected, and a second insert cannot reuse consumed
metadata. The populated 0179-to-0180 upgrade preserves old identity and content,
keeps metadata NULL, and rejects partial, desktop and short-prefix pairs.
Actual public HTTP tests cover
both posting routes with mobile metadata, including private storage and the
absence of these fields from responses. The initial staff fixture reused one
peer for consecutive posts and hit the existing five-second timer; distinct
synthetic peers in the same /16 now isolate metadata capture from that timer.

## Runtime work still required

BROWSER_HINT_KEY and its staff equivalent remain optional. When absent,
accepted posts retain no browser comparison metadata. Later event creation
must treat those rows as unavailable evidence; complete capture requires
configuring the key before accepting posts.
Services comparing the same posts need matching browser-hint and poster-identity
keys, even though each role reads its own configuration variables.

Moderation event creation must read this stored target evidence, never
browser IDs or prefixes submitted in moderator forms. Posting history stores
the actual thread ID for an OP, so the target lookup must recover parent zero.
The current custom-ban API has no template qualifier. Its global-scope flag
must not activate automatic events; the template workflow still needs work
under issue #220.
The source constructs anonymous state before filtering and reissues its cookie
on an automatic refusal. Fresh, resumed and idle-reset states can all reach that
branch. Action registration happens only after post insertion. Cookie issuance
on refusal is therefore separate from recording a successful action.

## Event storage and enforcement

The private creator enforces a 100,000-row limit. Issuance removes up to
1,024 expired rows before checking capacity. A private creator reads retained
target metadata and deduplicates the strict one-hour scope. It is deliberately
unwired from the custom-ban endpoint until template authority exists.

Posting reads at most 129 same-board, parent and keyed-prefix candidates; the
extra row detects overflow. A known browser further narrows the lookup. Missing
browser hints require a scoped lookup so an applicable event cannot be bypassed
by making that provider unavailable. Desktop and native IPv6 requests skip the
mobile IPv4 stage. Enforcement follows configured network rules and precedes
fixed identity checks, within the existing ordinary/test-board filter scope.

Event writes serialize with posting's shared policy lock. Database tests
observe contention in both directions and verify that a waiting post sees a
committed policy change. An automatic refusal explicitly rolls back the posting
transaction before a separate transaction rechecks the live event and increments
its bounded hit counter. Counter failure never admits the post. No raw address,
header or post text is stored in that counter.
This replaces the source's per-hit address log with aggregate telemetry.
The supplied source has no later reader for those hit records, but the counter
does not preserve its seven-day per-attempt audit history.

Active events survive target deletion. Tests also cover earlier content-log
rollback, configured-rule precedence, OP/reply scope, badged-staff exemption,
missing hints and expiry. Actual HTTP checks reject mobile posts through both
routes without registering their newly minted anonymous sessions. Database
lookup failures fail closed, an intentional security difference from the source.

The catalog check initially failed after dump/restore because it interpreted
a sequence's implicit ACL as a table ACL. The revised seal uses the sequence
code `s` for `acldefault`, as specified in the
[PostgreSQL 16 documentation](https://www.postgresql.org/docs/16/functions-info.html).
The populated historical cleanup, fresh installation and dump/restore tests
pass with that correction. The upgrade also rejects a runtime sequence grant.

The current event checkpoint passes 101 local database regressions across
posting, reports and transfers. Four rolled-back readiness mutations verify
function security, runtime execution grants, private reads and sequence access.
The production store/public/staff manifests pass locked, all-target, all-feature
checks and strict Clippy; the production public HTTP test also passes. Those
tests use an owned PostgreSQL TCP fixture, so they do not qualify production
database TLS transport.

Hosted qualification of the preceding metadata checkpoint exposed an older
file-deletion fixture whose saved JSON lacked the two new NULL fields. Its
expected rows now add only those reviewed keys. Both invented comparison data
and a changed original actor still fail the guard. The full file-deletion
upgrade, repeat-action, rollback and restore sequence passes locally.

The application has no live ban-request backend yet. Trusted Pass, local-auth
and janitor bypass authority also remain unfinished. Cosmetic badges and
browser hints cannot substitute for those permissions. The source thread-ban
path does not create these events, so it must not acquire that side effect.

## Refusal cookie state

Migration 0186 adds a private resume-only operation. It creates fresh zero-action
state or resumes an existing capability, including fingerprint changes and the
inclusive seven-day idle reset. It never increments action counts, fills the
pending-action buffer, updates the last accepted activity, or creates post/report
memberships. Existing state gets a one-year expiry; expired state cannot be
revived, including after a row-lock wait.

The public wrapper requires a live matching range event and a read-committed
transaction. The private operation accepts no counter or verification values.
Its definition and execution grants participate in the restriction seal. New
sessions use the existing finite capacity policy and bounded expired-row cleanup.
The existing opaque capability protocol replaces the original cookie codec.

After the posting transaction rolls back, hit recording and cookie storage run
in a separate transaction. A savepoint isolates cookie-storage failure so it
cannot erase the independent hit. The public response sets the anonymous cookie
only after a confirmed commit, with the existing HttpOnly, SameSite, production
Secure and no-store protections. A typed refusal outcome prevents unrelated
network errors with similar text from refreshing a cookie. Staff calls without
an anonymous context retain the refusal and hit without issuing that cookie.
Supplying verified, route-owned anonymous context for ordinary staff posting
remains unfinished; this is a remaining caller-parity gap.

If the event has expired or disappeared before refresh, or session storage is
unavailable, the post remains refused and no cookie is emitted. Fresh-session
capacity failure cannot turn a refusal into a post or register accepted activity.
