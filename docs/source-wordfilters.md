# Source wordfilters and admission hooks

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
The implementation applies the built-in transformations during posting
and retains a bounded typed result for subsequent rendering. Local database,
public-route and staff authorization checks pass. The built-in filter
checkpoint passed CI; its results remain in [the verification record](verification-wordfilters.md).
[Configured content rules](content-admission.md) now run during public posting
with bounded evaluation and private transactional effects, after raw/line and
required-subject checks and before final blank-content admission. Complete caller
ordering, staff exceptions, IP/ASN policy and file blacklist interfaces remain
unfinished under this issue. Each later checkpoint still requires its own CI.

The reference is the supplied `4chan-old` revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The audit reads source code and
public board configuration. It does not load the legacy application,
configuration, RPC helpers, database, passwords or private filter lists.

## Built-in selection and scope

`imgboard.php:139` checks `WORD_FILT`, then selects the board-specific PHP
file if present, otherwise `global.php`. `WORD_FILT` defaults to yes in the
supplied global configuration; `/b/`, `/s4s/` and `/news/` disable it. A missing board
file uses the global filter, rather than concatenating both filters.
The board-reference extractor now records the effective switch for every
supplied board: 79 enabled and three disabled. The regenerated fixture and the
unchanged board/RSS migration comparisons pass. A first comparison command
used a nonexistent migration-0046 RSS filename; the corrected command uses
the existing `0047_rss_feeds.sql`. No historical migration was changed.

| Source file | Active transformations, in order |
| --- | --- |
| `global.php`, `vg.php`, `vp.php` | Uppercase substring replacement, selected common word replacements, Unicode-aware content replacement |
| `ck.php`, `int.php` | Uppercase substring replacement and selected common word replacements; no Unicode-aware replacement |
| `asp.php` | Uppercase substring replacement, the three case-specific `finna` replacements, common replacements, Unicode-aware replacement |
| `v.php` | Global transformations, then the ordered console-term substring replacements |
| `test.php` | Global transformations, then two random choices for the active leet transformation |

Every supplied filter returns subjects and names unchanged. The call site
nevertheless invokes subjects when nonempty and non-anonymous names while
preserving the trip suffix. The common callback preserves unlisted case
variants and the matched terms absent from its replacement map. Its pattern
uses ASCII boundaries. The Unicode-aware pattern has different boundaries,
case folding and a two-to-four-letter suffix branch. The fixture covers both,
including combining marks, non-ASCII letters, homoglyphs and the source's
byte-length test for suffixes.

The `/test/` random transformation is active without a date condition in this
snapshot. It samples two choices from zero through five; equal choices apply
once. Its special `t` rule excludes lowercase `g` and `l` and consumes
non-overlapping character pairs. A leading `t` is unchanged. This behavior
differs from replacing every `t`.

## Posting order and saved representation

In the active posting path, name/trip construction and ban/quota checks precede
the content/IP admission filters and normalized/original-file blacklist check.
Content filters run for anonymous or non-capcoded posting and on the test
board. The file blacklist runs without a capcode. Those exceptions do not
disable the later built-in word transformations.

Comment sanitation first normalizes line endings, removes the source's fixed
Unicode exclusions and intra-word spoilers, shortens same-board numeric
quotes, and applies the board's spacing policy. Existing `~?rep?~` and
`~?erep?~` markers are removed once before content admission. Joining text can
form another marker; the later formatting pass removes that remaining marker.
Repeating the early cleanup would change the input seen by the wordfilters.

The code then prepares spoilers, SJIS, code and OP markup and checks final
subject/comment admission. `word_filter` runs at `imgboard.php:5804`, on the
escaped generated HTML, before internal link normalization, word wrapping,
quote coloring, final comment-regexp admission, flood/duplicate checks and
Robot9000 processing. Applying the filter to raw BBCode instead would change
which delimiters survive. In particular, the active test filter can change
generated tag/attribute names and escaped entities. The Rust posting path
prepares the source markup, runs the selected filter under the board-policy
lock, then finishes link normalization, wrapping and quote coloring before
Robot9000 admission. Later reads use the saved result and retain the original
wrapping decision. Changing a board's current policy affects future posts.

Migration `0063_post_wordfilters.sql` records all 82 original switches and
the board-file replacement policy. The source extractor checks that migration
suffix independently of the Rust selection logic. New synthetic boards have
filtering disabled by default. Historical posts retain a null payload and
their existing formatter; the migration does not rewrite their comments.

The `WF01` payload contains bounded UTF-8 text, four fixed changed-entity
spellings, breaks, eight approved markup kinds, two validated random choices
and one frozen wrapping flag. Decoding checks version, lengths, discriminator
values, UTF-8, balanced per-kind nesting and trailing bytes before rendering.
The templates escape ordinary text and choose generated tags and attributes
from closed values. They do not apply a template-safe HTML filter to saved
comment strings. A corrupt payload has one inert failure representation.

The final escaped source projection remains comment data for comparison and
Robot9000 processing. Search instead uses a bounded text projection of the
completed tokens, so generated markup and word breaks do not split a match.
Search excerpts clip those completed tokens to 1,024 visible scalars while
retaining surrounding finite markup. They do not rerun filters or normalize
only a clipped portion of the comment.

The insertion trigger stamps both payload and search text from transaction
context after staff authority validation. Public and staff roles cannot insert
or update those columns directly. The attachment owner receives only the two
board-policy read grants needed by this trigger. Staff tickets bind both exact
values through the existing account, session, board scope and revocation
checks; a changed value fails before consuming the ticket.

The older general `check_blacklist` function remains defined, but the audited
active call uses `check_md5_blacklist`; its definition alone is not evidence of
an active general-field check. The active file check considers normalized and
original MD5, board/global/work-safe scope, reject, auto-ban, DMCA-repeat and
quiet fake-success branches. Source security code and raw MD5 authority are
not a safe runtime extension mechanism for the Rust services.

## Configured admission behavior

`spam_filter_post_content_new` reads active global or board-specific rules from
the external `postfilter` table. It strips selected BBCode for matching and
builds normalized string, expanded regexp and OP autosage projections. Rules
have minimum match counts, lenient-known-user exceptions and OP-only scope.
The first matched rule records a hit, then may autosage, log, reject, auto-ban
or report fake success without posting. The query has no explicit ordering;
the replacement policy needs a defined order instead of inheriting database
row order. Database-query failure returns false in the source; the replacement
must specify and test its failure policy explicitly.

The Rust content-rule path uses operator-owned `admission.rules`, ordered by
`position,id`. It supports the audited literal/regexp decisions, minimum counts,
lenient activity and OP scope, then records only the selected hit, log or ban.
The policy lock remains held until commit. Updates wait for readers; stale
effect generations fail. Explicit rejection and quiet success retain those
effects without allocating a post. Ordinary post failures roll them back.
The source's unavailable-query allow branch is replaced by an error response.
See [the implementation and remaining scope](content-admission.md).

The filename proxy check runs before that query. A query failure also skips
the fixed subject checks that follow the rule loop. An OP autosage rule that
does not match its autosage projection still falls through to ordinary
matching; a later match of that rule returns autosage. Autosage takes
precedence over its log/reject flags. Content leniency requires a session
known or verified for the source's 24-hour check and more than ten posts.
Known status depends on password and network-mask ages, activity counts and
IP-change history, so a cookie's age alone is insufficient.

The active caller skips content/IP admission for authenticated capcoded staff,
except on `/test/`. MD5 admission has its own capcode condition. These
exceptions precede the built-in transformations and do not grant arbitrary
markup authority. String-rule normalization uses the source ICU transliterator
and a fixed ASCII whitelist. Its short-URL callback is commented out; the
active normalization path does not expand links through RPC.

Additional source checks include fixed subject restrictions, filename proxy
detection, IP/ASN range bans, known-user and verified-user exceptions,
browser/request signatures, password/country history, flood checks and optional
threat scoring. These depend on operator data and anonymous-session/Pass
state. No private rule rows, whitelist contents, histories or trained scoring
data are supplied by the public snapshot. Their absence does not authorize
inventing production lists or treating placeholder decisions as parity.

The image admission function scans bounded portions or the whole upload for
embedded code/archive signatures and rejects APNG. The rewrite's isolated
media admission and format-specific work must account for these branches;
copying its shell commands, deprecated regexp execution or URL-expansion RPC
is outside the replacement design. Related ban, session and media work remains
tracked separately.

## Current checks and resource limits

`scripts/extract-wordfilter-reference.php` starts a separate PHP process for
each of the eight audited, hash-pinned pure files. Only the two test-filter
random draws are replaced with fixed synthetic choices. Transformation code
remains unchanged. Workers do not load posting, postfilter, configuration or
RPC code. Worker output and errors are bounded.

The fixture records PHP 8.3.6 and PCRE2 10.42, source hashes, 738 field-scope
and transformation cases, and 108 vectors covering all 36 choice pairs on
three inputs. All listed console substitutions are exercised directly and
inside longer text. The Unicode word class deliberately follows the extracted
PCRE2 shape: letters, numbers and underscore. PCRE2 changed this class in
10.43; see its [pattern specification](https://www.pcre.org/current/doc/html/pcre2pattern.html).
The five source classes are pinned to that recorded engine rather than the
Unicode tables of a newer library. `extract-wordfilter-unicode-reference.php`
compares every valid scalar and records the sorted ranges, membership counts
and source hash. Rust uses those fixed ranges. This establishes class
equivalence to the recorded extraction environment, not to every historical
PHP or PCRE deployment.

The engine scans fixed built-in patterns and performs only ordered literal
replacements. Character membership uses binary search over the generated
ranges; custom operator regexes are not accepted by this module. Input is
capped at 131,072 bytes and output at
524,288 bytes. Random choices use the operating system source, unbiased byte
rejection, a fixed attempt cap and explicit failure, rather than PHP's shared
PRNG. Posting samples once, retains the choices and result, and rolls back on
failure. Raw board character admission precedes transformation, so a legal
replacement expansion does not acquire the raw input's smaller length limit.
The saved encoding and search projection are each capped at 131,072 bytes;
the final escaped source projection is capped at 524,288 bytes.

Thread reads count the selected comments, payloads and search text inside the
same repeatable-read transaction before transferring their bodies. Their
combined ceiling is 64,064,000 bytes, preserving the former maximum of 1,001
raw comments. Native updater reads use the smaller of twice their response
budget and that ceiling: 8 MiB with the default 4 MiB response cap. The updater
formats one post at a time before writing the bounded response.

Board and catalog snapshots also check selected bodies inside their read
transaction. Comment, payload and search text share the former 64,000-byte
ceiling for each selected OP/reply slot. The check uses the same latest-reply
limits as the body query. Metadata-only reads fetch no comment bodies.

On October 1, the initial source fixture comparison, domain checks and domain
Clippy passed on the isolated checkout. Later exhaustive comparison found
9,392 word-class differences, 9,272 letter-class differences and one missing
caseless character in the newer library-based implementation. The filter now
uses the source ranges, and the direct matching-library dependency is removed.
Regression vectors cover those boundary and caseless differences through all
profiles. Properties still exercise 128 bounded arbitrary-Unicode cases,
input/output limits and entropy failure/rejection bounds.

The second extractor, `extract-wordfilter-posting-reference.php`, runs only
hash-pinned pure sanitation, markup, filter and formatting functions on
synthetic inputs. It records 1,480 cases through caller cleanup, markup,
filtering, normalization, wrapping, quote coloring and both catalog teaser
modes. Every test-filter choice pair is covered. Boundary cases include nested
spoilers, tabs, private Unicode, joined internal markers, same-board quotes,
and the source's removal of incomplete trailing entities. Database tests
verify the imported policy, unchanged names and
subjects, locked policy changes, retained randomness, replacement expansion,
grant denials, Robot9000 duplicates and rejected attachment rollback. Public
route tests check pages, both JSON listeners, updater snapshots, catalog, RSS
and bounded search. All eight staff posting cases pass, including changed
payload/search text, replay, expiry, revocation and private discussion.

Fresh role bootstrap passes with historical payload and search columns null.
Integration uses a separate disposable database; applied migration checksums
are not edited when the new migration changes during development. The complete
workspace repeat passes all 626 tests and Clippy. The new desktop/mobile cases,
selected regressions and inline quotes pass as recorded in
[the verification record](verification-wordfilters.md).

```text
php scripts/extract-wordfilter-reference.php SOURCE fixtures/wordfilter-reference.json --check
php scripts/extract-wordfilter-unicode-reference.php SOURCE fixtures/wordfilter-unicode-reference.json crates/domain/src/wordfilter_unicode.rs --check
php scripts/extract-wordfilter-posting-reference.php SOURCE fixtures/wordfilter-posting-reference.json --check
python scripts/extract-board-reference.py SOURCE fixtures/board-reference.json --check --migration migrations/0045_original_boards.sql --rss-migration migrations/0047_rss_feeds.sql --wordfilter-migration migrations/0063_post_wordfilters.sql --board-encoding-migration migrations/0064_board_reference_encoding.sql
cargo fmt --all -- --check
cargo test -p board-domain --locked
cargo clippy -p board-domain --all-targets --all-features --locked -- -D warnings
cargo test -p board-store --test wordfilters --test robot9000 --all-features --locked
cargo test -p board-public --lib --test wordfilters --all-features --locked
cargo test -p board-staff --test posting --all-features --locked
npm run test:wordfilters
```
