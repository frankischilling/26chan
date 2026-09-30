# Post-number navigation

Click `No.` to visit a post's permalink. Click the digits to quote that post in
Quick Reply. Ctrl-click on the digits keeps the existing selected-text action
without inserting the post number. Other modified clicks retain browser
navigation. Only the original post's own header can open Quick Reply; copied
previews remain ordinary links.

When JavaScript or Quick Reply is disabled, the digits link opens
`/{board}/thread/{thread}?quote={post}#reply`. The server prefills the ordinary
comment field with `>>{post}` and a newline. The target must be a canonical,
positive i64 number belonging to a visible post in that thread. Invalid or
duplicate parameters fail with 400; a missing, deleted or foreign-thread post
fails with 404. Closed and archived threads reject the prefill and retain
permalink targets in their headers. GET requests do not post content or change
thread state. The HTML prefill parameter does not change the public JSON API.

## Permitted reference and adaptation

`Parser.buildHTMLFromJSON` in the pinned
[public extension v1191](https://s.4cdn.org/js/extension.min.1191.js) emits a
`postNum` wrapper with two anchors. Their titles are `Link to this post` and
`Reply to this post`; their labels are `No.` and the post number. On the
current thread, the original reply target is a JavaScript URL. Elsewhere it is
a thread URL with `#q`. The rewrite uses the same visible split and a bounded
same-origin form URL under its fixed-script policy. That fallback is an
explicit adaptation, not original-server behavior.

[The release manifest](public-watcher-assets.json) pins the client hash.
[The header record](public-post-header-reference.json) and
[badge asset manifest](public-capcode-reference.json) pin six desktop styles,
the inspected formatter branch and fixed icons. Only synthetic names,
subjects and comments enter the reference reproduction. No extension
initialization, public post import or external network request is allowed.

## Live posts and previews

The updater and preview parser accept the two links only as one complete
recipe in the matching post header. The board, thread, post number, titles,
labels and permitted closed-thread fallback must match exactly. Arbitrary
anchor titles, query links in comments, alternate threads, nested labels and
extra control attributes are rejected before DOM construction. The main
thread validates the worker's recipe again. Local quote copies preserve the
same finite links and omit forms, filled controls and duplicate IDs.

The native click handler leaves the permalink alone. It accepts only the
digits link from a matching `pc`, `p`, `pi` and thread container. Settings,
closed-thread guards, selected text and the existing draft lifecycle still
apply. A direct fallback URL also reveals the mobile ordinary form when
JavaScript is enabled but Quick Reply is disabled.

## Qualification

The production Askama fixture renders each of six staff badges on an OP and a
reply. Six theme checks compare all 72 cases with recorded public desktop
font, color, line-height, badge-icon, highlight and padding properties. The
comparison normalizes only font-family capitalization. Separate mobile checks
cover fit, the two controls and fixed 16-pixel icons at densities 1 and 2.
Those mobile captures are owned application fixtures, not original mobile
layout or full-page parity evidence. Founder keeps its available 1x icon;
the pinned public 2x URL returned 404 during the earlier collection.

Reproduce the public facts from the previously collected, hash-matched client
and six desktop CSS files:

```sh
node scripts/verify-public-header-reference.mjs /path/to/extension.1191.js /path/to/desktop-css
```

The verifier checks facts against the committed record. `--write` generates
the record for an inspected reference change; it does not update screenshots.
Pinned browser version, zero retries, pixel tolerances and parser/DOM limits
remain unchanged. See [the verification record](verification-post-number-navigation.md)
for actual results and hosted status.
