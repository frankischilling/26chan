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

The resolver preserves the pinned numeric key map, ignores INPUT/TEXTAREA targets
and Alt/Shift/Ctrl/Meta combinations, and prevents default/propagation for a
recognized unmodified shortcut. It does not add a blanket exclusion for all
focusable elements: the reference does not exclude SELECT or BUTTON targets.
Configuration is read for each event, and global disabling suppresses shortcuts.
The catalog does not install this extension shortcut listener.

Sibling links remain same-origin, same-board, query/fragment-free HTML page
targets. The client follows existing GET links rather than synthesizing form
submissions; report, deletion and posting forms are never used for pagination.
No new endpoint, script authority or arbitrary persisted command string is added.

## Explicitly unfinished native behavior

The reference also maps `A` to auto-updater, `Q` to Quick Reply and `R` to the
in-place thread updater, with feature/runtime guards. Those runtime features are
not implemented here. Their keys remain recognized but have no action while the
features are absent, and the help panel explicitly says so. A reload is not
substituted for an in-place update, and an ordinary posting form is not presented
as Quick Reply. The Quick Reply-specific shortcut group is likewise unfinished.

This is a completed watcher/filter/navigation integration step, not a claim that
all native shortcuts or the full extension are complete. The help uses the
existing accessible dialog styling and close/focus behavior; exact native panel
geometry and complete public-reference visual qualification remain open.

Unit tests cover opt-in/global flags, exact key mappings, modifiers/editable
targets and bounded sibling URLs. Persisted-browser tests cover Settings-based
opt-in, cross-tab watch changes, input preservation, selected-text filter entry,
actual pagination, index/catalog navigation and help/no-substitute behavior.
