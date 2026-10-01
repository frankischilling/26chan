# Source wordfilters and admission hooks

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
The isolated implementation currently provides and verifies the built-in
transformation engine. Posting integration, stored rendering, operator policy
and admission hooks are unfinished. The engine alone does not change the
running site's posting behavior or establish filter parity.

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

## Posting order that integration must preserve

In the active posting path, name/trip construction and ban/quota checks precede
the content/IP admission filters and normalized/original-file blacklist check.
Content filters run for anonymous or non-capcoded posting and on the test
board. The file blacklist runs without a capcode. Those exceptions do not
disable the later built-in word transformations.

The code then prepares spoilers, SJIS, code and OP markup and checks final
subject/comment admission. `word_filter` runs at `imgboard.php:5804`, on the
escaped generated HTML, before internal link normalization, word wrapping,
quote coloring, final comment-regexp admission, flood/duplicate checks and
Robot9000 processing. Applying the filter to raw BBCode instead would change
which delimiters survive. In particular, the active test filter can change
generated tag/attribute names and escaped entities. Stored rendering needs a
finite representation that preserves that phase without treating user text as
trusted HTML. That integration is pending.

The older general `check_blacklist` function remains defined, but the audited
active call uses `check_md5_blacklist`; its definition alone is not evidence of
an active general-field check. The active file check considers normalized and
original MD5, board/global/work-safe scope, reject, auto-ban, DMCA-repeat and
quiet fake-success branches. Source security code and raw MD5 authority are
not a safe runtime extension mechanism for the Rust services.

## Configured admission behavior still to implement

`spam_filter_post_content_new` reads active global or board-specific rules from
the external `postfilter` table. It strips selected BBCode for matching and
builds normalized string, expanded regexp and OP autosage projections. Rules
have minimum match counts, lenient-known-user exceptions and OP-only scope.
The first matched rule records a hit, then may autosage, log, reject, auto-ban
or report fake success without posting. The query has no explicit ordering;
the replacement policy needs a defined order instead of inheriting database
row order. Database-query failure returns false in the source; the replacement
must specify and test its failure policy explicitly.

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

The fixture records PHP 8.3.6 and PCRE2 10.42, source hashes, 714 field-scope
and transformation cases, and 108 vectors covering all 36 choice pairs on
three inputs. All listed console substitutions are exercised directly and
inside longer text. The Unicode word class deliberately follows the extracted
PCRE2 shape: letters, numbers and underscore. PCRE2 changed this class in
10.43; see its [pattern specification](https://www.pcre.org/current/doc/html/pcre2pattern.html).
The Rust character tables follow the pinned library's Unicode data; full
deployment/reference Unicode-version qualification is not claimed.

The engine scans fixed built-in patterns and performs only ordered literal
replacements. Two fixed, single-character Unicode classes use `regex` 1.13.1;
custom operator regexes are not accepted by this module. Compiled classes and
caches have explicit budgets. Input is capped at 131,072 bytes and output at
524,288 bytes. Random choices use the operating system source, unbiased byte
rejection, a fixed attempt cap and explicit failure, rather than PHP's shared
PRNG. Posting integration must retain the chosen result once and roll back
the post on failure.

On October 1, the source fixture comparison, all domain tests, formatting and
domain Clippy passed on the isolated checkout. Five new tests cover the
extracted transformations, all random pairs, input/output bounds and entropy
failure/rejection limits, plus 128 bounded arbitrary-Unicode cases across all
five built-in profiles. Initial checks on `regex` 1.12.4 passed; the module
was then pinned to the current 1.13.1 release and the complete domain command
and Clippy passed again. The [upstream changelog](https://github.com/rust-lang/regex/blob/1.13.1/CHANGELOG.md)
records the release. No runtime filter implementation is counted as complete
by these library checks.

```text
php scripts/extract-wordfilter-reference.php SOURCE fixtures/wordfilter-reference.json --check
cargo fmt --all -- --check
cargo test -p board-domain --locked
cargo clippy -p board-domain --all-targets --all-features --locked -- -D warnings
```
