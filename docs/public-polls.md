# Public poll browsing

Migration 0109 adds an empty-by-default, operator-owned poll projection. Public
pages can list published polls, show their ordered options, and display supplied
results. Voting is unavailable and is labeled as such. This does not complete
polls or issue #225.

## Source and missing evidence

The supplied snapshot `545b7812d1849f7958d914950c91fdbbe38f6b22` contains
`www.4chan/views/polls.tpl.php` and `polls-view.tpl.php`. Their links establish
`/polls`, `/polls/{id}` and `/polls/results/{id}`. They show a title, an optional
description, ordered options, per-option scores, percentages and a total count.
Missing scores display as zero.

The matching controller, schema and voting script are absent. The templates
cannot establish voter identity, repeat-vote policy, token validation,
publication rules, catalogue selection or the ordering query. The rewrite does
not infer those rules from anonymous posting sessions or Pass-only banner votes.
It adds no vote endpoint, token, voter record, successful placeholder submission
or production poll data.

The `polls` board name is reserved for these site routes. The supplied board
inventory does not use it. Migration 0109 rejects an existing conflicting board
atomically; an operator must resolve that configuration before upgrading. It
never renames or deletes that board automatically.

## Owned publication and reads

The existing migrator owns `poll_private.polls` and `poll_private.options`.
Runtime roles cannot read or write these tables. The public role can select only
the two publication-filtered, security-barrier views. Every options read applies
the parent poll's publication filter, even outside the application.

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

## Public interface

The three paths accept GET and HEAD. Unknown, unpublished and invalid poll IDs
return 404. Other methods cannot submit votes or change configuration. The home
page links to Polls. Options remain a readable list with navigation to results;
there is no working-looking vote form.

Titles, descriptions and captions are escaped as text. Pages load no poll
script, remote analytics or voting token. Poll paths are excluded from generic
board-page classification, so `/polls/1` does not acquire native board scripts,
posting destinations or session behavior. Existing response budgets and security
headers apply. Public readiness checks the projection's schema and privileges
without reading private data.

Complete original styling, voting authority, operator publication tooling and
actual production records remain separate work. This projection is a read-side
foundation, not a claim of a working source-equivalent voting system.

## Qualification

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

## Browser regression coverage

`npm run test:polls` exercises the real public server and a disposable migrated
PostgreSQL database at desktop and mobile widths. The fixture uses explicit
operator-owned rows; it does not activate voting or populate production polls.
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
