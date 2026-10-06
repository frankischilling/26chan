# Automatic-session equality for admission

Migrations 0095 and 0096 add forward-only equality for registered automatic
sessions. The supplied source obtains its posting password from `UserPwd`, not
from a submitted recovery-password field (`imgboard.php:4889-4895`). A fresh
UserPwd generates a random credential, retains it during cookie refresh and idle
reset, and can later replace it through verified sign-in. This implementation
covers the automatic-session branch. Verified-email replacement, Pass identity,
legacy cookies and authenticated staff integration remain separate work.

## Private identity and provenance

Each successfully registered session receives a database-generated random UUID.
It is an equality key, not a password, cookie, public poster ID, deletion proof
or staff credential. Neither a request nor a public database function accepts a
chosen UUID. Matching submitted recovery passwords never merge sessions.
Existing Argon2 recovery and per-post anonymous deletion proofs are unchanged.

New posting-history and report-membership rows capture that private identity.
They have no session foreign key: collecting expired activity does not remove
captured equality. A collected or unknown capability still cannot recover its
old identity through a guessed token, matching address or recovery password.

Existing sessions acquire an identity only on their next successful
registration. Historical posts and reports remain NULL. Migration adds nullable
transaction-provenance columns first and installs their defaults separately, so
it cannot mark old rows as newly created. Registration requires the membership
to have been inserted in the current transaction and not already claimed. An
old unregistered row cannot acquire either this identity or anonymous deletion
authority through a later registration call. This deliberately tightens the
older direct database API.

Activity validation precedes allocation. Post/report registration, identity
allocation and history capture commit together; rejection rolls them all back.
A valid existing identity survives activity updates and the seven-day idle
reset. Existing session, posting-history and report-membership capacity limits
still apply; adding equality columns introduces no unbounded identity registry.

## Active-thread quota

The public posting path includes automatic identity only when the source would
consider UserPwd established. Source `isNew()` compares the creation second
with the current request second (`lib/userpwd.php:649-651`). Therefore:

- A newly minted session checks IP only.
- Reusing an existing cookie in its creation second still checks IP only.
- A request that triggers the exact seven-day idle reset checks IP only.
- A subsequent request can match the retained automatic identity again.

The quota resolves this from the locked, pre-registration state. It does not
substitute `!minted`, advance activity to manufacture age, or accept a caller's
`isNew` flag. Each live OP is counted once when its IP or eligible automatic
identity matches. The original stored-post-time cutoff, board-specific maximum
and period, zero-limit behavior and error wording remain unchanged.

Public posting already owns the session lock through content admission before
checking the quota. The new resolver reenters that lock and adds no identity
gate or cross-board lock. Staff and internal callers without a genuine public
session retain the existing IP-only path. Source verified-email and Pass
identity matching are not inferred from staff authentication.

## Report boundary

Migration 0095 captured automatic equality on new report memberships. Migration
0097 now enforces [IP OR automatic identity report decisions](source-report-admission.md).
Reports include a known identity even on creation-second and idle-reset
requests. Their board → report-gate → session order is preserved; GET uses only
an existing verified capability and cannot allocate identity or refresh activity.
The public mutation requires full session context and performs registration
inside its atomic SQL call. Staff retains a separate IP-only mutation.

## Rollout and verification

Stop writers, apply through 0097, then start matching public and staff services.
No new bootstrap role is required. Older public binaries still call the IP-only
quota API, so a binary-only rollback does not preserve this new quota policy.
Both readiness endpoints inspect the new
helpers, six-argument quota API, private column grants and provenance schema
without reading identity rows. Runtime roles cannot read or directly stamp the
private identity or transaction-provenance columns.

The upgrade script covers populated 0094 upgrades, fresh databases, historical
NULL preservation, current dump/restore, restricted APIs and readiness drift.
Store cases cover cross-address equality, overlapping predicates, exact newness
boundaries, rollback, session collection and rejected historical adoption.
Exact commands and results are recorded with each pull-request checkpoint.

A compromised application can still lie about request context or use a session
token hash it possesses. The database protects its private equality namespace;
it cannot independently authenticate an HTTP cookie from supplied arguments.
Server-side serialized activity also differs from the original client-cookie
snapshots. Neither boundary is presented as complete identity parity.
