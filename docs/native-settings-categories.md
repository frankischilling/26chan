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

Disclosure is captured once at page startup. A successful raw storage read of
missing settings or an empty string selects the source first-run presentation;
stored `{}` and other nonempty values start collapsed. Failed reads do not prove
absence. Reopening uses that same disclosure flag while reading current checkbox
values. Changes from another tab do not replace an open draft or recapture the
page's presentation flags.

Presentation flags remain separate from initialization. On a page that proved
settings were absent or empty at startup, opening Settings saves the finite
source Config defaults through the shared preference lock. Page loading alone
does not write them. Reopening that same document keeps first-run disclosure;
a later navigation sees the saved preferences and starts collapsed.

Before each opening write, the application reads current storage again. Explicit
values from another tab, hidden preferences and safe unknown keys survive the
merge. Malformed, oversized or unavailable data is left untouched. This avoids
the source's stale full-Config overwrite. Unsupported compatibility flags remain
inactive. Storage or locking failures retain the existing same-tab fallback and
do not claim a successful persistent write.

First-run mobile-device navigation defaults use the source user-agent test,
independently of viewport layout: dropdown navigation is enabled and top-page
navigation is disabled. Mobile layout applies its source defaults for YouTube,
compact threads and linkification. Persistent navigation is suppressed while a
non-catalog page has mobile layout, without clearing its saved preference.

Opening persistence has its own cancellation signal. Closing Settings, saving,
leaving the page or completing a reviewed restore cancels a queued opening write.
Export waits for its settlement. Initialization does not acknowledge watcher
reads, emit an explicit Save event or reload the page. A pending restore review
blocks initialization, and a successful restore invalidates startup defaults
before releasing the lock so a sparse reviewed result stays sparse. Incoming
restore links still require explicit review and confirmation.

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

Settings availability and its layout-dependent checkbox overrides use the same
startup layout snapshot. Resize and never-mobile preference changes do not alter
that page's Settings matrix; fresh navigation recaptures it. Other responsive
page features keep their existing live behavior. This is a Settings presentation
contract, not complete parity with every use of the source's global layout flag.

Category buttons retain `aria-controls` and `aria-expanded`. Initial focus must
be visible even when categories are collapsed. Escape, cancellation and opener
focus restoration retain their existing contracts.

## Evidence

The focused browser suite checks exact category and option order, disclosure
state, cancellation, source desktop/mobile visibility, startup disclosure and layout snapshots,
retained hidden preferences and save behavior.
Existing watcher/settings and theme workflows exercise the integrated dialog.
Browser results belong to the exact tested commit; local static or unit checks
alone do not establish a rendered browser pass.

At `2fd227b`, all four Windows theme shards and the main Windows job passed;
the latter includes all 15 focused Settings browser cases. Linux passed its Rust
and database checks, then failed the six-theme page-filter editor test. Its trace
shows the desktop editor working, followed by a fresh 390px page whose Settings
correctly exposes only thread hiding in the Filters category. The test still
waited for the desktop-only editor link.

The corrected fixture retains the desktop editor, palette, nested help and
focus checks. Its mobile branch checks the source control matrix and verifies
that a saved filter remains effective despite its editor being absent. This
fixture correction requires a new hosted browser run; the preceding 15 passing
filter cases do not qualify the changed case.

The first-open persistence slice adds pure default/parser checks and isolated
cancellation/Save/Export cases, plus integrated cross-tab, storage-failure,
mobile-device/layout and sparse-restore tests. Local qualification passed 23 pure
Settings/layout cases, generated-asset checks, two public binary tests and 121
public library tests. One Unix-socket library case is excluded locally after the
executor returned `EPERM`; it remains enabled in CI. Browser-dependent local
checks could not launch the configured browser, so their registered bodies are
unverified until hosted execution. Test discovery lists 32 isolated Settings
cases and 48 integrated watcher, navigation and transfer cases.

At `3940ad4`, the corrected filter suite passed all 16 cases. Linux then reached
an updater test that opened returning-user Settings and tried to check
`alwaysAutoUpdate` without expanding Monitoring. The retained trace shows the
expected collapsed categories. The fixture now uses the shared Settings opener
that explicitly expands Monitoring; updater assertions and timeouts are
unchanged. Windows, all four theme shards and media/operations passed on that
revision, but this Linux failure means the overall run did not pass.
