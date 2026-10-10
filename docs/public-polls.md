# Public polls and voting

Published polls have a catalogue, ordered options, native voting forms and results.
Migration 0128 adds voting to the publication model introduced in 0109. Existing
polls stay closed until an operator enables voting. No production polls or votes
are seeded. Feedback, staff applications and banner contests remain open under
[#225](https://github.com/frankischilling/26chan/issues/225).

## Source and presentation

The supplied snapshot `545b7812d1849f7958d914950c91fdbbe38f6b22` contains
`www.4chan/views/polls.tpl.php` and `polls-view.tpl.php`. Their links establish
`/polls`, `/polls/{id}` and `/polls/results/{id}`. They show a title, an optional
description, ordered options, per-option scores, percentages and a total count.
Missing scores display as zero.

The options template supplies a POST form named `poll-form`, radio fields named
`id`, a hidden `_ptkn`, and an `action=vote` submit button. It puts the same token
in `body[data-tkn]` and links to Back to Polls and View Results. The rewrite keeps
those fields and destinations and gives radios accessible names through their
caption cells. Clicking a caption leaves selection unchanged, as in the source.
The source radios have no `required` attribute. An empty
selection reaches the server and returns 422 without changing votes or receipts.
The form works with JavaScript enabled or disabled.

The supplied archive does not contain the matching controller, schema, voting
script or poll stylesheet. The templates reference
[`polls.css?15`](https://s.4cdn.org/css/polls.css?15) and
[`polls.js?4`](https://s.4cdn.org/js/polls.js?4); both were recovered on 2026-10-10.
These are the bytes returned by those versioned URLs on that date, not proof of
their contents when the supplied snapshot was made. The recovered script only
copies the body token into an `_ptkn` cookie on form submission. It does not
render controls, send asynchronous votes or require a selected option.

Poll pages have a separate layout and stylesheet. The header, catalogue intro,
table geometry, colors, native button, navigation and footer follow the supplied
templates and recovered CSS. Results put the percentage and count in the colored
left cell, the caption in the right cell and the total in the final table row.
An empty catalogue omits the Current Polls heading and table. The original pages
have no viewport meta element; that mobile layout behavior is preserved.

Only the poll rules from the recovered stylesheet are shipped. Its background
points to the existing local `fade.png`, whose bytes match the recovered image.
Pages do not load board styles, theme controls, analytics or external scripts.
Operator text is escaped and can wrap at the storage limits. Caption references
give radios accessible names without changing their appearance. The signed HttpOnly cookie
replaces the script-written source-domain cookie. Historical closed polls retain
an explicit read-only options page because no source controller was supplied.
The cookie, token, duplicate-vote and publication rules below are documented
rewrite decisions; the templates and recovered script do not establish the
original backend rules. Footer destinations that are not implemented remain
separate work under [#194](https://github.com/frankischilling/26chan/issues/194).

### Reference fixtures

`tests/fixtures/polls` retains both original PHP templates, the recovered CSS and
script, synthetic inputs, and six generated HTML pages. The raw captures are
preserved byte for byte. SHA-256 digests are:

| Input | SHA-256 |
| --- | --- |
| `polls.tpl.php` | `e75ae9df16eee5e471a1000155ffa14bf3170aaae084c8f76fecf241ff49037d` |
| `polls-view.tpl.php` | `4281918d8734fdfaf5758f8963b0453ff857a2ed6d0cc171c1741c8704c2fd14` |
| `polls.css?15` | `6baf99fbbb66468b8e10f1c9b333c942cd49b6d22570c6edbf6f988b4da275fb` |
| `polls.js?4` | `4b480887bc189760a5dbb38bbe8422e6964c69470b5791a2bf0eff1155cb33d6` |
| `fade.png` | `5f7a2be79027d3a5c7207de3e7efe510bcc4a66f105e174d1000cbffd6e4a274` |

Run `php -n -d allow_url_fopen=0 -d allow_url_include=0 scripts/render-poll-reference.php`
to regenerate the reference HTML, or append `--check` to verify it without writes.
The script checks input digests before evaluating the two reviewed templates.
It removes their exact unavailable footer include, fixes the request year at
2026, removes analytics and the cookie-writing script, relocates the stylesheet
and favicon, and normalizes HTML line endings. It does not recreate the source
markup from the Rust templates. The fixture stylesheet retains all captured CSS
rules and only relocates the fade image.

`npm run test:themes -- tests/themes/poll-source.spec.js` compares complete page
pixels and native form/link contracts against the production views in the same
browser. It covers populated and empty catalogues, options with and without a
description, results with a missing score, and empty results at 1280 by 900 and
390 by 844. The mobile cases use a mobile browser context. Both screenshots are
retained on a mismatch. Linux CI also regenerates and checks the PHP references;
the existing Linux and Windows theme jobs run the page comparisons. These cases
qualify the supplied templates plus the captured assets, not an unseen original
controller or historical asset version.

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

`npm run test:polls` includes closed-poll reads and active voting at desktop and
mobile widths, with JavaScript both enabled and disabled. Its four browser cases
check missing-selection rejection, two independent votes, repeated submissions,
closed states, escaping, overflow, allowed requests and stored totals. The source
page comparisons above qualify appearance separately. Current qualification and
hosted CI results are tracked in [#225](https://github.com/frankischilling/26chan/issues/225).

Fixtures require explicit development mode and a loopback migrator connection.
Their random ownership markers and IDs constrain setup, inspection and cleanup.
Cleanup runs after callback failure and uncertain setup results, retains both
errors when cleanup also fails, and removes the owned receipts through the poll's
foreign key. All 13 fixture unit cases pass using a process stand-in for those
guards; PostgreSQL and browser tests establish the actual storage and page
behavior. Rust formatting and strict Clippy across the workspace, targets and
features pass. Every local qualification cluster was stopped and removed.

The earlier read-model migration checks remain part of qualification. They cover
populated 0108-to-0109 upgrades, a conflicting `polls` board, and administrator
dump/restore with exact row, definition and privilege comparisons. PostgreSQL
rejects mutations of the joined options view with 55000 before checking grants;
the exercise independently requires absent write privileges and accepts that
code only for that view. Other denied operations still require 42501. The local
adaptation uses loopback TCP; hosted qualification uses isolated Unix sockets.
Fixtures use synthetic rows and no real poll or voter data.
