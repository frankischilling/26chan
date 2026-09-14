# Source subject cleanup

Issue: https://github.com/frankischilling/26chan/issues/137

New subjects follow `imgboard.php:5299-5388` and `lib/postfilter.php:71-153`
in the supplied source. The 100-byte UTF-8 input limit applies before cleanup,
even when every character would later disappear. Unlike comments, subjects
always run the finite ASCII-lookalike mapping and zero-width removal, including
on `/a/`, `/jp/` and SJIS boards. The existing source emoticon ranges follow;
SJIS still preserves its box-drawing range. Runs of two or more ASCII/fullwidth
hashes and U+2318 are removed by the fake-capcode stage; single hashes survive.
The early blank-only class contains spaces, ideographic spaces and literal pipes,
but not tabs.

The shared source spacing stage then applies the board's ordinary/code/SJIS
policy and PHP-style ASCII trim. Subject CR/LF removal follows that stage, then
removal of codepoints above U+3134F. Neither trim nor fake-capcode cleanup repeats:
removal can expose edge spaces or adjacent hashes that remain stored. Subjects
do not use comment quote rewriting, intra-word spoiler cleanup or line admission.
Name cleanup, tripcodes, forced-anonymous policy, required-subject/text-only board
switches, word filters and ordinary OP subject-or-comment admission remain
separate unfinished work. This change does not establish complete posting parity.

## Representation and authority

Subjects remain untrusted text in PostgreSQL and escaped Askama templates.
The read-only JSON `sub` field uses the source's `ENT_QUOTES` representation:
`&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#039;`. Existing entity-looking input is
escaped, not decoded or trusted. `json.php:303-527` retains nonempty subjects
on replies as well as OPs; both posting and JSON now preserve that behavior.
Empty subjects omit `sub`. Server HTML (`imgboard.php:2333-2341`) and the source
extension (`js/extension.js:839-853`) display subjects only on OPs, even when
a reply has a retained JSON subject. The shared HTML/updater template follows
that distinction. Historical stored values are not cleaned on reads,
although JSON now applies this source text representation to them too.

Removing fixture reply subjects shortens their headers and changes reply width
and wrapping. The six attachment board/thread/archive desktop/mobile baselines
cover that change. Source-exact comment wrapping is still unfinished; the current
`overflow-wrap: anywhere` fallback can make text beside a wide desktop thumbnail
narrower than the source layout. These synthetic baselines are not a claim of
complete page geometry parity.

The public handler validates early, while the authoritative normal and approved
attachment insertion paths prepare raw input once under the board transaction
lock. Rejected input changes no post counters or thread clocks and does not
consume an approved receipt. Unsupported controls and malformed UTF-8 retain
the rewrite's explicit rejection policy instead of PHP's ambiguous repair.
No user HTML becomes trusted markup, and no dependency or runtime grant is added.

A 100-byte subject containing tabs can expand beyond the old 120-byte storage
ceiling. Migration 0029 raises that independent ceiling to 400 bytes. For example,
`A`, 98 tabs and `B` become 394 bytes with code/SJIS spacing. Catalog search
serialization retains the full expanded value. The application still checks
100 raw bytes; a compromised public database login can insert up to the separate
400-byte database ceiling, but cannot remove it or rewrite existing subjects.

Apply migrations through 0029 before deploying the binary. Existing rows and
thread clocks are unchanged. Keep the wider constraint during binary rollback
while expanded subjects exist; do not silently truncate them. Older binaries
may reject expanded new writes or omit reply subjects. Backup/restore preserves
stored text, and rollback cannot recover characters removed during posting.

## Verification

Domain cases cover unconditional normalization on the source exception boards,
all spacing policies, fake-capcode boundaries, private-codepoint order, raw and
expanded limits, exact entity spelling and 128 generated Unicode inputs.
Database/HTTP cases exercise both aliases and form encodings, retained replies,
escaped HTML/JSON, observed board-lock waits, historical values, expanded OPs and
unchanged rejection clocks. Approved-attachment cases retain receipt rejection
and reuse checks. A no-JavaScript browser case checks expanded native posting,
reply subjects in JSON but not HTML, and full catalog search fields; the watcher case
checks decoded subject text without adding HTML elements.

`sudo bash scripts/test-subject-migration.sh` upgrades an owned disposable
database through 0028 then 0029, retains historical text/clocks, checks actual
public-role 394/400-byte inserts and 401-byte rejection, and denies subject/schema
mutation before removing that database. Local compilation and visual fixtures
do not establish these database, migration or posting-browser results. Complete
current-head hosted qualification is required before merge. Production
containment, recovery and independent review remain launch prerequisites.
