# Native Settings categories

The non-catalog Settings dialog groups its existing controls in the supplied
source order:

1. Quotes & Replying
2. Monitoring
3. Filters & Post Hiding
4. Navigation
5. Images & Media
6. Miscellaneous

The reference is `SettingsMenu.options` and its disclosure handlers in
`4chan-old/js/extension.js:8966–9214`, revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. This restores grouping and ordering;
it does not establish complete Settings parity.

Each heading controls one category. Expand All opens every rendered category
and remains an expand-only action. Neither action saves preferences. Unsaved
checkbox changes survive collapsing a category and are discarded when the
dialog closes. Reopening rebuilds the initial disclosure state. Existing option
keys, labels, defaults, editors and dirty-key-only saving remain unchanged.
The separate catalog editor is unaffected.

## Preserved behavior and remaining differences

The rewrite opens all categories when its current settings object is empty;
otherwise it starts with them collapsed. Opening the dialog does not save
settings. The source instead captures first-run state at page initialization
and saves defaults on first opening. That first-run persistence difference
remains.

Current mobile visibility is preserved. It is not the source's complete mobile
option matrix. In particular, the rewrite supports a desktop dark-theme control
that the supplied source marks mobile-only. Missing `unmuteWebm` and
`forceHTTPS` controls are not added by this change.

The rewrite selects layout when the dialog opens and keeps an open draft intact
across resizing. The source captures its layout flag once during page startup.
Reopening after a layout change can therefore differ. These visibility and
lifecycle differences remain separate work.

Category buttons retain `aria-controls` and `aria-expanded`. Initial focus must
be visible even when categories are collapsed. Escape, cancellation and opener
focus restoration retain their existing contracts.

## Evidence

The focused browser suite checks exact category and option order, disclosure
state, cancellation, unchanged desktop/mobile controls and save behavior.
Existing watcher/settings and theme workflows exercise the integrated dialog.
Browser results belong to the exact tested commit; local static or unit checks
alone do not establish a rendered browser pass.
