# Automatic mobile-range policy

Issue #212 includes automatic mobile-range events created by moderation. The
domain evaluator and event planner are implemented and tested. Posting and
moderation do not yet call them; there is no runtime event store or trusted
per-post browser metadata capture.

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

The creation planner accepts a previously authorized global, non-warning ban
or ban request. A direct ban records a range event unless a matching one is
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

## Runtime work still required

Accepted posts need private server-derived metadata before moderation can
create events from a trusted target. Moderator form fields must never supply
the browser ID or prefix. The existing posting history stores the actual
thread ID for an OP, so a target lookup must explicitly recover parent zero.

Thread moves preserve source browser metadata and change reply scope to the
destination thread. Existing source-scope events remain on their original
board and parent. The separate source copy operation drops browser metadata.
Any new metadata table needs corresponding transfer evidence, catalog checks,
private grants and deletion handling.

Event writes must serialize with posting's shared policy lock. A rejected post
must roll back earlier admission effects; recording a hit cannot commit its
posting transaction. Active events must survive deletion of their target for
their remaining lifetime. Database failures in the source lookup are ignored;
the planned fail-closed runtime behavior is an intentional security difference
that still needs qualification.

The application has no live ban-request backend yet. Trusted Pass, local-auth
and janitor bypass authority also remain unfinished. Cosmetic badges and
browser hints cannot substitute for those permissions. The source thread-ban
path does not create these events, so it must not acquire that side effect.
