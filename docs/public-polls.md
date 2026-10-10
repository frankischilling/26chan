# Public polls and voting

Published polls have a catalogue, ordered options, native voting forms and results.
Migration 0128 adds voting to the publication model introduced in 0109. Existing
polls stay closed until an operator enables voting. No production polls or votes
are seeded. Feedback, staff applications and banner contests remain open under
[#225](https://github.com/frankischilling/26chan/issues/225).

## Source and missing evidence

The supplied snapshot `545b7812d1849f7958d914950c91fdbbe38f6b22` contains
`www.4chan/views/polls.tpl.php` and `polls-view.tpl.php`. Their links establish
`/polls`, `/polls/{id}` and `/polls/results/{id}`. They show a title, an optional
description, ordered options, per-option scores, percentages and a total count.
Missing scores display as zero.

The options template supplies a POST form named `poll-form`, radio fields named
`id`, a hidden `_ptkn`, and an `action=vote` submit button. It puts the same token
in `body[data-tkn]` and links to Back to Polls and View Results. The rewrite keeps
those fields and destinations, adds labels and required selection, and accepts
the form without JavaScript.

The matching controller, schema, voting script and poll stylesheet are absent.
The cookie, token, duplicate-vote and publication rules below reconstruct that
missing backend contract. They are explicit rewrite decisions; the templates do
not prove the original rules. Exact original styling is still unqualified.

The `polls` board name is reserved for these site routes. The supplied board
inventory does not use it. Migration 0109 rejects an existing conflicting board
atomically; an operator must resolve that configuration before upgrading. It
never renames or deletes that board automatically.

## Owned publication and reads

The existing migrator owns `poll_private.polls`, `poll_private.options` and the
new private vote receipts. Runtime logins cannot access these tables directly.
Public reads use two publication-filtered, security-barrier views. Every options
read applies the parent poll's publication filter, even outside the application.
A separate NOLOGIN function owner holds the narrow grants needed to record votes.

Publication and ordering are explicit operator decisions. A published poll may
have no catalogue ordinal and remain accessible by its detail link. Catalogue
ordinals are unique and range from 1 to 200. Option ordinals are unique within
a poll and range from 1 to 128. Reads preserve that order. Detail metadata and
option scores share one repeatable-read snapshot.

The following are rewrite safety bounds, not recovered source limits: 512 UTF-8
bytes per title, 16,384 per description, 1,024 per caption, positive signed
64-bit IDs, and integer counts or scores from zero through one billion. Scores
may be absent. No invariant that scores sum to the supplied total is inferred
from the missing controller. Read limits reject oversized results rather than
silently returning a truncated catalogue or option set.

Percentages use integer arithmetic, round positive hundredths half up, and omit
trailing decimal zeros. A zero total displays zero percent. These are explicit
bounded presentation rules; they do not establish arbitrary legacy PHP/database
numeric identity. Imported results must be reviewed under their actual source
contract before publication.

## Public interface and voter credentials

All three paths accept GET and HEAD. `/polls/{id}` also accepts the native vote
POST. Unknown, unpublished and invalid poll IDs return 404. The home page links
to Polls. A closed poll remains readable and has no voting form. HEAD does not
mint a cookie or form token. An active poll with no options or missing option
scores cannot offer a new voting form until an operator supplies that data.

Opening an active poll creates or reuses a signed random browser credential.
It uses the `board-poll` cookie in development and `__Host-board-poll` in
production, with HttpOnly, SameSite=Strict, Path=/ and a one-year lifetime.
Production also requires Secure. The credential is independent of anonymous
posting sessions, IP addresses, Pass credentials and staff accounts.

Three distinct HMAC keys are derived from `POSTER_ID_KEY`: one authenticates
the browser credential, one authenticates the form, and one hashes a poll's
voter receipt. A form expires after 30 minutes, or earlier when its cookie expires,
and is bound to that exact cookie and poll. Tokens reject future issue times,
invalid signatures, noncanonical encodings and timestamps outside the allowed
range. Changing the key invalidates old cookies and forms. The system clock must
be correct. Neither credential nor token is logged by the voting path.

The cookie represents one browser identity. Clearing it, changing browsers or
waiting for it to expire can create another identity that may vote again. This
is not proof that each vote came from a different person. Key rotation also
creates new identities; operators must account for that before reopening polls.

The private receipt contains a poll ID, a per-poll voter hash and a timestamp.
It stores no selected option, raw cookie, token, IP address or posting identity.
The same browser produces different receipt hashes for different polls. A repeat
submission acknowledges its existing vote and leaves all totals unchanged, even
if the submitted option changed or the poll closed after the original vote.
An active poll revisited by its voter displays results and a recorded-vote notice.

Successful and duplicate submissions return 303 to `/polls/results/{id}`.
A vote with an invalid, expired or mismatched credential returns 403; malformed fields or
an unavailable option return 422; a closed or full poll returns 409. Ambiguous
duplicate cookies return 400, and form bodies over 1,024 bytes return 413.
Unpublished polls remain 404 for reads and writes. Missing signing configuration
or changed database authority makes voting unavailable with 503.

Titles, descriptions and captions are escaped as text. Poll pages run no scripts,
workers, embedded documents or remote analytics. Their CSP permits same-origin
native form submission and blocks script execution and script network calls.
Personalized responses use `private, no-store`; existing Origin, Fetch Metadata
and request-budget checks also apply. Poll paths remain outside board-page
classification, including single-digit detail paths.

## Atomic votes and operator setup

`content.cast_poll_vote` locks the published poll first, then its options in a
fixed order. It validates the selected option and supplied scores, inserts one
unique receipt, and increases the chosen score, total and new-vote count in the
same transaction. An error rolls everything back. Concurrent submissions from
one browser count once. Operator closure, unpublication and option changes are
serialized with voting; an option from another poll cannot receive a vote.

`accepting_votes` defaults to false and `new_vote_count` to zero. Historical scores
and totals are preserved. Each poll allows 10,000 new receipts by default;
operators may set `vote_capacity` from 1 to 100,000. The existing one-billion
numeric bounds still apply. These limits are rewrite storage bounds, and are
checked while holding the same poll lock as the vote. Operators may close a poll
without deleting its results or receipts. Deleting an owned poll cascades to its
options and receipts.

Fresh installations create `board_poll_owner` through `deploy/roles.sql`. For an
existing installation, an administrator must apply `deploy/poll-role.sql` before
migration 0128. The migrator has SET-only membership; runtime logins have none.
The owner has no login, role administration, schema creation, direct post access
or broad table-write authority. `board_public` receives only EXECUTE on the two
voting functions. Public readiness inspects catalog metadata for their definitions,
ownership, grants, schema and constraints without reading private votes.

An operator enables an explicitly published poll only after supplying its options
and scores and choosing a capacity. The rewrite does not silently reopen imported
polls, invent historical receipts or change supplied aggregates. Dedicated operator
publication screens remain separate work; the current model uses administrator
database operations and disposable local fixtures.

## Voting qualification

Local qualification passes on fresh PostgreSQL 16 databases. The 0128 migration
exercise covers missing-role rollback, the existing-installation bootstrap,
historical row preservation and readiness under the actual public login. The
earlier poll-read upgrade and dump/restore exercise and complete fresh role
bootstrap also pass. Hosted CI must qualify each published commit before merge.

The domain suite passes, including signed-cookie and form vectors, tampering, expiry, future
timestamps, canonical encodings, cross-poll binding and key rotation. Store tests
exercise real restricted roles, duplicate races, closure and option locks,
capacity, missing scores, transaction rollback and readiness drift. All four
voting and three existing read tests pass. Both public HTTP integration cases pass
and cover native forms, cookie and token binding, request limits, publication states,
results and exact fixture retirement.

`npm run test:polls` includes the earlier closed-poll read checks and active voting
at desktop and mobile widths, with JavaScript both enabled and disabled. All four
browser cases pass. They check required selection, two independent votes,
repeated submissions, closed states,
escaping, overflow, allowed requests and the stored totals. These are functional
browser checks; no original poll-page screenshot baseline is claimed.

Fixtures require explicit development mode and a loopback migrator connection.
Their random ownership markers and IDs constrain setup, inspection and cleanup.
Cleanup runs after callback failure and uncertain setup results, retains both
errors when cleanup also fails, and removes the owned receipts through the poll's
foreign key. All 13 fixture unit cases pass using a process stand-in for those
guards; PostgreSQL and browser tests establish the actual storage and page
behavior. Rust formatting and strict Clippy across the workspace, targets and
features pass. Every local qualification cluster was stopped and removed.

## Earlier read-only qualification

Three store integration cases and the public HTTP case pass on a fresh database.
They cover explicit order, publication isolation, escaped maximum-sized output,
percentage boundaries, method denial, real runtime privileges, readiness drift
and a concurrent operator update across one read snapshot. The public library's
127 supported cases, the domain suite, formatting and strict workspace Clippy
also pass. The known local Unix-socket exclusion remains enabled in CI.

Populated 0108-to-0109 and fresh installation exercises pass, including rejection
of a conflicting `polls` board without partial changes and administrator
dump/restore with exact row, definition and privilege comparisons. PostgreSQL
rejects mutations of the joined options view with 55000 before checking grants;
the exercise independently requires absent write privileges and accepts that
code only for that view. Other denied operations still require 42501.

The local script adaptation uses loopback TCP; hosted qualification runs its
original isolated Unix-socket setup. On `6e31833`, the full Linux job and
media/operations job passed. Poll-specific rendered-page comparisons remain
unqualified. No real poll or voter data is used.

On `6e31833`, Windows passed its preceding checks and eight of ten public-state
cases. The two empty-directory comparisons changed exactly where expected: the
new Polls link and the resulting downward movement of the directory/footer.
The retained desktop (1280 by 900) and mobile (390 by 844) captures were inspected
beside their original baselines; neither shows overflow or unexpected content
or layout changes. Pixel comparison also finds ten changed glyph-edge pixels
in unchanged top text in each viewport; their cause is not established by this
capture. The subsequent exact-pixel run on `1d80a98` passed both captures.
Only those two baselines were updated from the actual hosted PNGs. Exact pixel
comparisons remain enforced. All ten public-state cases passed in the
[Windows job](https://github.com/frankischilling/26chan/actions/runs/37589648036/job/112690920590).
The full build, monitoring and dependency checks also passed on that head;
this does not explain the earlier glyph-edge change or establish poll-page parity.

### Earlier read-only browser checks

The original read-only suite exercises the real public server and a disposable
migrated PostgreSQL database at desktop and mobile widths. Its fixture uses
explicit operator-owned rows and leaves voting closed.
The checks cover catalogue and option order, escaped hostile text, options and
results navigation, visible content, overflow, inert controls, restricted
requests and unchanged stored results. These are functional browser checks,
not comparisons against original-page screenshots.

The fixture requires explicit development mode, a loopback `board_migrator`
connection and the normal browser harness prerequisites. Setup commits its rows
atomically; cleanup matches this run's random ownership markers and IDs, including
after an uncertain setup result. Test and cleanup failures are both retained.

Local validation passes all eight fixture-unit cases, JavaScript and shell syntax
checks, Rust formatting and Playwright discovery of both browser cases. The
existing generated-asset checks and 22 deletion-quota fixture cases also pass.
On a fresh PostgreSQL 16.15 database with all migrations applied, the real poll
fixture passes its success and callback-failure paths. Both retire their owned
polls/options while preserving an unrelated poll and its result count.

The browser cases have not run locally; the qualified browser environment is
unavailable. Test discovery and the real SQL fixture check do not establish
rendering or navigation behavior. Hosted CI must qualify both cases against the
published commit. The aggregate verification script includes the new checks. The Linux CI job allows 75 minutes because its previous successful run
took 59m45s before these additions; per-test timeouts and assertions are unchanged
for the existing suites.

The first hosted run on `2de4676` reached both poll browser cases and passed their
rendering, navigation, visibility, overflow and inertness checks. Its final network
assertion rejected the normal local `fade.png` background. The allowlist includes
that exact same-origin image; methods, query strings, resource types, unexpected
requests and HTTP failures remain checked. The current local qualification above
passes both complete closed-poll browser cases as well as active voting.
