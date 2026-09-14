# Native post-menu event and recursive-helper evidence

## Pinned evidence

The permitted public source is
`https://s.4cdn.org/js/extension.min.1191.js`, collected on
2026-09-13 at 12:55:46.721 UTC, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
It was inspected as text, not executed. The previously captured public thread
HTML identified by `public-watcher-navigation-reference.json` was inspected only
for script references and the recursive-control attribute, not imported as post
content or executed.

`PostMenu.open` builds the menu, marks its trigger active, then dispatches
`4chanPostMenuReady` on `document` before attaching the menu to the document.
`UA.dispatchEvent` uses an ordinary Event with both bubbling and cancellation
disabled, and assigns a `detail` object. The three payload members are `postId`
(a string), `isOP` (a boolean) and `node` (the live menu UL, not a copy).
Subscribers can augment that list before the client measures and places it.

## Implemented interface

The fixed client publishes the same event shape and lifecycle point for local
board and thread post menus. It builds the complete menu before notification,
then attaches and measures it, including subscriber additions. Keyboard focus is
chosen from the resulting visible menu items. Closing a menu, refreshing watch
state or globally disabling the extension does not emit a new ready event.

No external script is loaded, no new CSP permission is granted, and no event
payload is converted into HTML. Subscriptions are a same-document interface;
they do not authorize third-party network access or automatically bind arbitrary
subscriber command attributes to backend actions. Existing report, deletion and
watch controls retain their actual handlers and authorization boundaries.

Three persisted-browser tests cover OP/reply identity, event class and flags,
the detached live-node identity, complete initial menu contents, subscriber
augmentation before keyboard focus and placement, and absence of duplicate
events on close, cross-tab refresh and disabling.

## Recursive hiding: distinguish helpers from exposed behavior

The pinned extension defines `ReplyHiding.toggleR`, `hideR`, `showR` and
`shouldToggleR`, plus the `4chan-hide-rr-<board>` storage key. Their existence
alone is not evidence of a built-in recursive-hiding control.

In this exact script, `data-recurse` occurs once, in the generic click dispatcher.
The generated reply menu uses ordinary `data-cmd="hide-r"` without that attribute.
The only call to `toggleR` is that conditional dispatcher branch; the only calls
to `hideR` and `shouldToggleR` occur within `toggleR` itself. The captured thread
HTML contains no `data-recurse` attribute. Thus the inspected inputs do not
establish a built-in recursive menu or parse-time recursive restoration path.

The menu-ready event could support an external subscriber that adds such an
attribute, but third-party subscriber behavior was not observed or executed.
This is an unresolved extension-integration question, not verified native UI
parity. No recursive menu item or automatic recursive hide is invented from the
dormant helper definitions. The local ordinary-reply implementation leaves the
separate recursive storage key untouched.

Earlier draft notes listed recursive hiding as unfinished native UI work before
this reachability check. This evidence corrects that inference; it does not
claim complete page parity, compatibility with every third-party extension, or
completion of the broader rewrite.
