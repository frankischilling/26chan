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
Visibility follows the source metadata for the supported controls.
The separate catalog editor is unaffected.

## Preserved behavior and remaining differences

The rewrite opens all categories when its current settings object is empty;
otherwise it starts with them collapsed. Opening the dialog does not save
settings. The source instead captures first-run state at page initialization
and saves defaults on first opening. That first-run persistence difference
remains.

Supported mobile controls follow the source category matrix:

- Quotes: quote preview, backlinks and Quick Reply
- Monitoring: updater, automatic updates, watcher, automatic watching and statistics
- Filters: thread hiding
- Navigation: thread expansion and infinite scroll
- Images: image expansion, spoiler reveal and thumbnail hiding
- Miscellaneous: linkification, dark theme, Custom CSS, ID colors and local time

Disable All remains outside the categories. Dark theme is mobile-only in the
Settings UI; a saved dark-theme preference still applies on desktop. Hidden
preferences remain stored and are not submitted or cleared when visible options
are saved. The catalog editor retains its separate controls.

Source sub-option styling applies only to automatic watching, traditional board
navigation, auto-hide navigation and the image-hover background option.
`unmuteWebm` and `forceHTTPS` remain inactive compatibility values, not working
controls. Playback support remains incomplete. Enforced server transport policy does not
use this inactive preference; the visibility change does not add either control.

The integrated application supplies the source YouTube default through its
option callback. An isolated dialog fixture's fallback defaults are not proof
of the complete application defaults.

The rewrite selects layout when the dialog opens and keeps an open draft intact
across resizing. The source captures its layout flag once during page startup.
Reopening after a layout change can therefore differ. These visibility and
lifecycle differences remain separate work.

Category buttons retain `aria-controls` and `aria-expanded`. Initial focus must
be visible even when categories are collapsed. Escape, cancellation and opener
focus restoration retain their existing contracts.

## Evidence

The focused browser suite checks exact category and option order, disclosure
state, cancellation, source desktop/mobile visibility, retained hidden preferences and save behavior.
Existing watcher/settings and theme workflows exercise the integrated dialog.
Browser results belong to the exact tested commit; local static or unit checks
alone do not establish a rendered browser pass.
