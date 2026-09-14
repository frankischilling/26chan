# Native keyboard shortcuts: watcher, filtering and navigation

The pinned public extension v1191, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`,
defines `Keybinds.init`, `Keybinds.resolve` and the Navigation Settings option
`keyBinds`. The option defaults to false. The source is
`https://s.4cdn.org/js/extension.min.1191.js`, collected on
2026-09-13 at 12:55:46.721 UTC and inspected as text, not executed.

## Implemented behavior

Settings exposes the native optional shortcut checkbox and Show link. The
watcher, filtering and navigation shortcuts use actual existing actions:

- `W`: watch/unwatch the current thread when the watcher is enabled; no board-index watch is invented.
- `F`: pass the current selection to the existing filter editor when filters are enabled.
- `I` and `C`: navigate to the current board's index and catalog.
- `B` and `N`: follow the server-rendered previous and next page links when present.
- `R`: fetch and insert new replies through the bounded [thread updater](native-thread-updater.md) on active thread pages.
- `A`: toggle the same updater's automatic countdown, subject to thread/updater/busy-state guards.
- `Q`: open [Quick Reply](native-quick-reply.md) on an eligible thread page when that feature is enabled, quoting selected text without a post ID. It does not choose a thread from the board index.

The resolver preserves the pinned numeric key map, ignores INPUT/TEXTAREA targets
and Alt/Shift/Ctrl/Meta combinations, and prevents default/propagation for a
recognized unmodified shortcut. It does not add a blanket exclusion for all
focusable elements: the reference does not exclude SELECT or BUTTON targets.
Configuration is read for each event, and global disabling suppresses shortcuts.
The catalog does not install this extension shortcut listener.

Sibling links remain same-origin, same-board, query/fragment-free HTML page
targets. The client follows existing GET links rather than synthesizing form
submissions; report, deletion and posting forms are never used for pagination.
No arbitrary persisted command string is added. The updater uses the existing
public-only snapshot route and fixed native bundle without expanding CSP authority.

## Explicitly unfinished native behavior

The supplied `js/extension.js:8251-8254,8303-8335,9999-10003` also defines the
Q selection behavior, help groups and Ctrl-click quote action. Help lists the
Global keys in source order, then Quick Reply's always-enabled Ctrl+Click,
Ctrl+S and Esc shortcuts. Ctrl-click quotes selected text without linking the
post ID; Ctrl+S inserts spoiler tags and Esc closes the editor. Those three
actions do not require keyBinds, but Quick Reply itself must be enabled.
The complete Quick Reply lifecycle remains unfinished; the
[Quick Reply scope](native-quick-reply.md) records those limits.

This is a completed watcher/filter/navigation integration step, not a claim that
all native shortcuts or the full extension are complete. The help uses the
existing accessible dialog styling and close/focus behavior; exact native panel
geometry and complete public-reference visual qualification remain open.

Unit tests cover opt-in/global flags, exact key mappings, modifiers/editable
targets and bounded sibling URLs. Persisted-browser tests cover Settings-based
opt-in, cross-tab watch changes, input preservation, selected-text filter entry,
actual pagination, index/catalog navigation and help/no-substitute behavior.
