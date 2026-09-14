# Source comment word breaks

The supplied `4chan-old/imgboard.php:4594-4631` defines `wordwrap2`; its active
call is at 5830-5843. `4chan-old/lib/util.php:95-105` applies the UTF-8 regular
expression. After link generation, each text section outside generated HTML
tags is split on ASCII spaces. Every group of 35 Unicode scalars receives
`{{w_br}}`, including an exact final group. The final replacement changes that
marker to `<wbr>` after quote formatting. This supplies an optional line-break
position; it does not force a line break when the text fits.

The Rust formatter follows those boundaries with finite `WordBreak` tokens.
Generated markup and link boundaries reset the count. A link's destination
remains unchanged while its displayed label receives breaks. Quote-number
matching follows wrapping, so a break can terminate the numeric run. The
greentext span retains its break tokens because the source replaces the marker
after its quote pass. HTML remains escaped text; no caller chooses tag names
or attributes. The public and staff templates emit only attribute-free `<wbr>`.

ASCII space is the separator. Nonbreaking spaces and combining characters
count as scalars, not grapheme clusters or display columns. Prepared line
breaks and generated tags end text sections. An existing literal `{{w_br}}`
also becomes a break, even in a short comment. If wrapping splits that literal
marker, its remaining text fragments are preserved. The PHP early-exit scan
does not change insertion positions: every decoded 35-scalar run that can gain
a break necessarily satisfies its encoded non-space scan. The Rust pass avoids
that redundant scan. Work remains bounded by the existing parser input ceiling
and URL normalization bounds.

The format stamp selects behavior at insertion:

| Stored profile | Rendering |
| --- | --- |
| 0 | Original legacy formatter |
| 8-15 | Earlier spoiler/code/SJIS profile |
| 24-31 | Earlier profile with OP markup |
| 40-47 | Source word breaks with the saved ordinary markup bits |
| 56-63 | Source word breaks with the saved OP markup bit |

Migration 0036 extends the constraint and makes the existing invoker trigger
stamp the new version bit. It does not change historical rows, board policy,
timestamps, secrets or grants. The board lock, fixed search path and existing
OP-ownership hint retain their prior meaning. Both public posting and scoped
attachment insertion receive the stamp. Public SQL cannot supply or update
the format column, and a privileged insert supplying an old value is still
stamped with the current profile. Later board-policy changes do not change a
post's stored rendering.

Apply 0036 after 0035, then deploy the public and staff binaries and rebuilt
native filter/updater bundle together. Existing browser pages need a reload to
use the expanded worker grammar; the asset response requires revalidation.
The updater accepts only an attribute-free `wbr`; event handlers, styles and
classes on that element remain rejected. Catalog/search text concatenates the
displayed fragments without inserting spaces. Public JSON and updater HTML
use the same rendering profile as the page.

For binary rollback, retain the additive constraint and stamp function.
Earlier binaries treat the new profiles as bounded escaped text with literal
formatting markers. They do not preserve the new rendering, so a rollback must
record that loss. Do not silently rewrite saved profiles to simulate an older
deployment. There is no new operator flag because the source rule is always
active for ordinary posting.

Local checks cover scalar/space boundaries, trailing breaks, literal markers,
preserved destinations, quote ordering, all profile bits and bounded randomized
text. Renderer checks retain historical output and escape hostile text. The
actual HTTP matrix covers both posting aliases, encodings and response modes,
both JSON routers, updater fragments, saved policy and denied public changes.
The populated upgrade exercises all 17 historical profiles, new runtime stamps,
scoped insertion, OP bits, unchanged data/clocks and cleanup in an owned
disposable PostgreSQL database. Its four SQL blocks also run unchanged through
the guarded Linux harness in CI.

Browser cases post with JavaScript on and off, verify live replies containing
breaks, reload saved content, check link destinations and catalog text, and
capture desktop/mobile rendering. These images are inspected without changing
existing visual baselines. Full current-head CI remains required before merge.

The September 14 native Windows check passed the populated upgrade's four SQL
blocks, the scoped attachment integration test, both browser cases and all 16
updater parser/transport tests. The 1280px and 390px captures were inspected:
long runs wrap on mobile, remain inline where space permits, and literal HTML
stays visible as text. No existing screenshot baseline changed. The generated
client bundle check and formatting check passed. Linux-only proxy, service and
containment qualification still require the current-head hosted run.

This slice changes wrapping after the current link-selection stage. Source URL
normalization/linkification, word filters and unresolved quote-resolution rules
remain tracked separately under #143 and the compatibility inventory. It does
not establish complete formatting parity or qualify a production deployment.
