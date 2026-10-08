# Server comment links

New posts use saved formatter profiles 104 through 111 and 120 through 127. Migration 0043
adds the source-link policy while retaining the existing spoiler, code, SJIS,
OP markup and word-break bits. Historical profiles remain unchanged. Public
credentials cannot choose or overwrite the saved profile.

The source's `normalize_links` converts supported board thread, reply, legacy
PHP and catalog URLs into quote syntax. Same-board post references retain
`>>` labels; other boards retain `>>>/board/` labels. Fragment precedence,
case-sensitive probes, static-board allowlists, catalog search encoding and
rules references follow the pinned functions. Numeric labels retain leading
zeros while their destinations use bounded positive IDs.

Boundary handling follows the source's byte-mode pattern, including its wildcard
before `php`, ASCII whitespace and PHP's falsey `0` captures. The static-link gate recognizes signed exponent notation without converting
to a bounded float. Non-decimal or incomplete numeric terms reaching post resolution remain
literal: the rewrite does not resolve the source database's numeric coercions
or link only the leading digits to a different post. This is a remaining
post-resolution compatibility gap. As in the source, wrapping happens first;
a break can end a reference before an exponent, leaving a decimal prefix. Rules links retain lowercase alphanumeric board names independently of
routing-slug limits. Bare host slashes and trailing punctuation remain outside
server anchors. Entity-escaped input still has its own normalization boundary.

The additional [boundary fixture](../fixtures/source-link-boundary-reference.json)
contains 136 independently extracted cases: 62 normalization, 10 escaped-input
normalization, 49 static-link and 15 server-link cases. It checks the exact pinned
source bytes without loading the application. The escaped-input cases establish
normalization behavior only, not full HTML identity. Another 20 cases execute
the original post-wrap numeric gate with a recording missing-post stub. They
check the lookup term, not database coercion or existence.

The server links remaining HTTP(S) URLs only for the source's `4chan.org`,
`4channel.org` and `4cdn.org` hosts. It retains their lexical spelling. Other
URLs stay escaped text. The native Linkify URLs preference controls browser
decoration, including its desktop/mobile defaults and live insertion. Changing
that preference never changes persisted comments, server HTML or JSON.

Board, catalog and rules references use local routes. Ordinary post references
use the existing bounded post-resolution route. New-window links add
`noopener` and `noreferrer`; user text never supplies HTML or executable URLs.
Staff previews use the same parser, escape labels and keep quote navigation
inert. The original rules page and remaining dead-quote behavior are part of
#194 and #208, respectively.

Catalog search reconstructs the source's stored-comment representation before
post-number resolution. Static-link spelling contributes to the `/b/` length
check; ordinary numeric references remain text. Search metadata retains the
source attribute spelling independently of the safe attributes rendered on
the page. Both GET and live matching consume the prepared subject/teaser value.

[The synthetic fixture](../fixtures/format-reference.json) records the checkout
hashes and PHP/UTF-8 environment. Its 35 link cases execute the original pure
functions, and 52 teaser cases execute the source's word wrapping, tag removal,
SJIS replacement and truncation. No original request handler, database or
configuration is loaded. Source warnings are retained in the fixture.

```bash
php scripts/extract-format-reference.php 4chan-old fixtures/format-reference.json --check
cargo test -p board-domain --locked
cargo test -p board-public --test server_links --test word_breaks --all-features --locked
npx playwright test tests/browser/static-quotes.spec.js tests/browser/native-linkification.spec.js
```

Database tests require the disposable migration and public test connection
variables. Browser tests use the regular public process. These checks cover
source-link behavior and the stated catalog transforms; the broader rewrite
remains open in #191.
