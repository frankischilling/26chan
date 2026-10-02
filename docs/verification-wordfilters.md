# Wordfilter posting verification

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
This record covers the implemented built-in posting transformations. It does
not qualify the external
postfilter, blacklist, classifier or known-user admission branches, or the
complete rewrite. The pushed rewrite head including these changes still needs CI.

On integrated head `9ad321e`, the Windows visual job passed, including all
1,173 theme cases. The media/operations job failed while setting up the restore
exercise: its direct historical-fixture insert used the original `/test/`
board without supplying the now-required wordfilter result. The runtime guard
correctly rejected that insert. The fixture now uses the separate unfiltered
`/fixture/` board. The corrected restore exercise passes locally against fresh
PostgreSQL 16.15, with actual SQLx migration records, data fingerprints, restored
capabilities and existing restricted-role denials. It accepts a validated test
port so this disposable exercise can run beside the retained qualification
cluster; the default port and cluster-ownership check are preserved. Local
setup attempts with an incorrect operator command and an incorrect expected
migration count stopped before the restore checks and are retained. CI must
qualify the pushed correction.

## Reference and comparison

The operator-supplied source is pinned to revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The extractors verify the audited
file hashes before running pure functions in separate PHP workers. They do
not load the legacy application, private configuration, database or RPC code.
The fixture records PHP 8.3.6 and PCRE2 10.42. Synthetic inputs contain no
production content or credentials.

The field fixture contains 738 cases plus 108 test-filter vectors covering all
36 choice pairs. The posting fixture contains 1,480 cases through caller
sanitation, markup, filtering, normalization, link construction, wrapping,
quote coloring and catalog teasers. It includes the hash-pinned pure sanitation
functions from `lib/postfilter.php`; it does not load the external admission
functions or application initialization. The Rust comparison uses the extracted
outputs, rather than generating expectations with the Rust implementation.
The board extractor also verifies the original board, RSS and new wordfilter
migration policy.

The source mutates generated tag names, attributes and entities on `/test/`.
The replacement retains their finite meaning as typed components. Templates
escape ordinary text, including literal HTML, and choose approved components
directly. The source projection is comparison data and has no template-safe
HTML authority. Later reads do not sample random choices again.

## Completed local checks on October 1

The separate disposable database applies the current migrations without
editing previously applied migration checksums. Its public database permission
is revoked, with connect grants assigned explicitly to the test identities.
The runtime checks use their actual restricted roles.

- Fresh role bootstrap passes. It preserves historical comments, checks null
  historical payload/search fields, imports 79 enabled and three disabled
  original policies, denies runtime writes to the two saved columns, and grants
  the staff wrapper only to the authentication role.
- Workspace Clippy passes with all targets and features and warnings denied,
  including the browser helper and aggregate snapshot read bounds.
- The final all-feature workspace repeat passes all 626 tests, with no failures
  or ignored cases. Its subsequent workspace Clippy check passes as well.
- Domain tests pass: 69 unit cases and 43 integration cases. Properties exercise
  bounded arbitrary Unicode and arbitrary saved bytes, plus malformed versions,
  lengths, choices, discriminators, nesting, truncation and trailing data.
- All four wordfilter store tests pass. They check source policy, field scope,
  locked policy changes, retained choices, replacement expansion, direct grant
  denials and duplicate rejection after filtering. Exact full/tail read budgets
  pass; a one-byte-short budget rejects before transferring selected bodies.
  A rejected attachment post keeps its approved upload available. All three
  Robot9000 store regressions passed in the preceding integration run.
  Board/catalog reads reject expanded data above the selected OP/reply budget,
  while a page selecting a smaller post and metadata-only reads remain usable.
- All eight staff posting cases pass. Changing either the exact payload or
  search text fails without consuming the bound ticket. Replay, capacity,
  expiry, revocation, staff badges and private discussion remain covered.
  The private latest-post polling test also passes with its saved typed fixture.
- The public library's 109 tests and all four wordfilter route tests pass. The
  latter check ordinary JSON-negotiated posting, pages, both JSON listeners,
  updater HTML, catalog responses, RSS, unchanged saved rendering after a policy
  change, and late normalized search matches containing escaped script text.
  Catalog comparison covers both teaser modes for all 1,480 source cases. The
  original `/test/` retains its active profile; generic fixtures use a separate
  unfiltered board. Caller-order cases check all five profiles against the PHP
  output and require blank rejection with no inserted post.
- Client unit/updater checks pass 48 cases, and seven browser DOM files pass 86
  cases. After expanding the source corpus, the two worker tests and the release
  DOM test pass again with 432 markup vectors. They check every saved Test
  choice, local previews and copied HTML/text, and reject extra attributes,
  classes, resources and generated components moved into a post header.
- Generated release bundles match their pinned inputs. Native filter and
  backlink bundles remain within their existing 256/32 KiB ceilings.

The final new desktop/mobile suite passes all ten cases. The regression run
passes 234 cases, and all ten cases in the affected suites pass after the
fixture corrections below, covering all 242 selected regressions. All 36
inline-quote cases then pass with full Chromium. These results use no retries
and retain the failed attempts. The browser fixture
creates and removes owned synthetic boards with one of the five fixed profiles;
it does not alter an original board's filter policy.

## Failed attempts and corrections

Early prototype checks exposed a mistaken short-code expectation, a moved
validation error constant and an excerpt query that did not account for the
chosen leet replacement. Those were corrected before the completed domain
run. The first new staff test also used the wrong existing store signature;
the corrected test calls the six-argument staff API.

An attachment test refused the new database with `UnsafeRole`. The disposable
database had inherited PostgreSQL's default public temporary-table permission.
The fixture now revokes public database privileges and grants explicit connect
permissions. The same attachment and role checks then pass; no runtime guard
was weakened.

A staff rendering assertion expected named entities, while Askama emits numeric
entities. The corrected assertion still requires the exact escaped script
text. Excerpt checks also caught the difference between pre-normalization text
and the completed comment. Search now stores bounded completed text and clips
completed typed tokens. Its tests include normalized quotes, removed source
markers and arbitrary Unicode bounds.

Added truncation vectors found an incomplete changed entity at the end of a
catalog teaser. Inspection of `truncate_comment` establishes that the source
removes the entire incomplete entity. The formatter now does the same and all
expanded source cases pass. Clippy also rejected an unnecessary owned string
comparison in the new HTTP test; the test now compares the actual string field.

The first browser launch timed out at its unchanged 120-second startup limit
while Cargo waited for the full workspace build's artifact lock. No browser
case ran in that attempt. Subsequent browser checks found the hidden desktop
form toggle on mobile and a rejected saved Test class in the updater grammar.
The test now uses the actual mobile control; the client admits only the finite
generated source recipes inside a post comment. It still rejects unknown
attributes and header placement. A local launch also mixed two Playwright
installations; the runner now uses the checkout's own installation. All ten
cases then passed sequentially. Timeout limits, assertions, retries and
screenshot tolerances have not been relaxed.

Full workspace attempts exposed two private staff fixtures that inserted raw
comments into boards whose source policy enables filtering. Both now save a
typed fixture and search text through the same transaction context. The private
polling fixture creates its account, session, thread and posts in one setup
transaction. Another compilation caught the new read-limit error missing from
the staff capacity mapping; it now maps to that existing unavailable response.

Expanding comparison to the caller path caught an extractor omission: nested
spoilers are simplified before the built-in filters. The extractor now runs
the audited sanitation functions and source caller branches before filtering.
Additional cases cover tabs, line endings, private Unicode, same-board quotes
and markers formed by joining text. The independent caller comparison
then passes. A new route assertion initially expected HTTP 422 for a JSON
posting-rule error; the source uses HTTP 200 with only an `error` body. The
corrected test requires that exact body and unchanged row count.

The generic browser fixture move also needed escaped URI expressions and two
terminal Global Search board paths. Those destinations now match `/fixture/`.
The Unicode-limit scenario remains unchanged on the original `/test/` board
and still compares its limit with the independent board-source fixture. A
mechanical audit of 40 tracked fixture files verifies changes only to
board identifiers, custom navigation lists, URI encodings, storage keys, the
helper's Cargo target override and Rust formatting.

The expanded browser run passes all ten new wordfilter cases and 234 of its
242 regressions. Eight failures identify fixture defects: the custom list
still used a space-separated `test` identifier, the tail helper ignored the
configured Cargo target, and the owned browser profile directory was absent.
The failed log and eight traces are retained. The corrected fixture lists,
target lookup and local setup pass all ten cases in the affected suites;
timeouts, assertions, retry settings and screenshot baselines are preserved.

An interrupted workspace repeat coincided with the Windows system drive
having about 43 MB free. The WSL instance subsequently restarted and its
temporary database disappeared. Cleaning only regenerable native Cargo
package artifacts recovered about 12 GB. The interrupted log is retained;
it contains compilation output and no completed test results. Qualification
restarts with a fresh owned database, whose cluster data now lives outside
the temporary directory. No cause of the WSL restart has been established.
The first resumed run stopped before executing `post_number_links` with an
OS executable-format error. A header audit found three damaged executables,
all from the public package, including its visual fixture. Cargo clean removed
that package's regenerable artifacts before rebuilding. Source and reference
data were preserved. The failed run is retained as incomplete evidence.
The next run passed 614 tests before the monitoring fixture rejected the
retained cluster location. Its marker guard had required `/tmp`. The fixture
now admits both generated disposable roots, with exactly eight alphanumeric
tag characters. Its actual restricted-role and unsafe-grant checks then pass,
alongside the final board-bound, concurrency, public library and route checks.

The final reference check also found an extractor encoding error. Reading
the source's board names without an explicit encoding had decoded the UTF-8
name "Pokémon" as "PokÃ©mon" on Windows. The extractor now specifies UTF-8.
Migration 0064 corrects that title only when it still has the imported value;
it preserves migration 0045's checksum and an operator's edited title. The
reference check verifies both the historical SQL and the additive correction.

## Bounds and remaining qualification

Raw posting admission still allows at most 16,000 Unicode scalars and 64,000
bytes, subject to the smaller board limit. Filter input/output budgets are
131,072/524,288 bytes. Saved `WF01` data and completed search text are each
limited to 131,072 bytes. Decoding allows at most 32,768 parts and checks each
closed discriminator before rendering. Search clips display to 1,024 visible
scalars while retaining finite surrounding markup.

Full and tail snapshot reads count selected comment, payload and search bytes
inside their repeatable-read transaction before fetching bodies. The combined
ceiling is 64,064,000 bytes. Native updater reads use twice their bounded output
budget, capped by that ceiling, and format one post at a time. Budget rejection
does not mutate content.

Board/catalog reads check the combined selected body bytes before transferring
them, with a ceiling of 64,000 bytes per selected OP/reply slot. Their regression
uses a legal maximum-size Unicode post whose three saved projections exceed
the OP-only budget. It also checks a page selecting a smaller post and a
metadata-only read. These checks and workspace Clippy pass; the complete
workspace repeat passes all 626 tests. The browser results are recorded above.

An exhaustive source-class audit found 9,392 word-class differences, 9,272
letter-class differences and one missing caseless character in the newer
matching library. The filter now uses fixed source ranges and removes its
direct regex dependency. The new fixture records all five classes over
1,112,064 scalars with PHP 8.3.6/PCRE2 10.42. The independent fixture and the
runtime range lookup agree for all five classes over every scalar. The local
workspace and selected browser checks pass. Equivalence to a
different historical engine is not claimed.
External operator rules and classifier data were not supplied and are not invented.
Their safe configuration, admission ordering, staff exceptions and failure
policy remain implementation work under the open issue.

Hosted [run 36962497073](https://github.com/frankischilling/26chan/actions/runs/36962497073)
on the restore correction `e669a41` passed media/operations and the complete
Windows visual job. Linux exceeded its 40-minute job limit while its browser
suite was still progressing; GitHub's check annotation confirms the timeout.
The aggregate gate failed. The Linux job limit is now 60 minutes, with all
assertions and required checks retained. This newer checkpoint still requires
its own full CI qualification.
