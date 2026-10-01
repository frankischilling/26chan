# Catalog Settings

The catalog Settings editor now supplies the five options in the pinned public
client v1025: Disable keybinds, Don't spoiler images, Open threads in a new tab,
Thread Watcher and Use drop-down navigation. It also restores the shortcuts
list, Custom CSS field, close icon and Save Settings action. Desktop-only rows
are hidden on mobile. Saving applies the reviewed values without reloading.

The sparse `catalog-theme` object holds `nobinds`, `nospoiler`, `newtab` and `css`.
False options and empty CSS are omitted; clearing every field removes this key.
Watcher and navigation flags remain in `4chan-settings`, preserving unrelated
values. The public client's unsaved CSS draft survives closing and reopening
the editor. Saving or clearing CSS uses the stored text on the next opening.

Navigation follows the independently recorded catalog defaults: absent native
storage leaves it off, while an admitted empty object enables it on desktop.
An explicit false or `disableAll` keeps it off. Mobile hides the navigation
option and its initial effective flag is false, including when stored true.
The editor focuses its first visible checkbox inside a native modal; Escape
and the close icon return focus to the opener.

## Reference

[The replay](../scripts/verify-public-catalog-settings-reference.mjs) runs the
whole unchanged released catalog client on three owned cards. It verifies the
public script and six stylesheets against the existing byte/hash pins, denies
all nonfixture requests and checks page errors at every record and page close.
No production post, filename, media or private checkout supplies its fixtures.

[The record](public-catalog-settings-reference.json) contains 24 editor states,
16 native-default states and 12 computed-style cases across widths 1280/390 and
six themes, alongside the separately qualified [keyboard cases](catalog-shortcuts.md).
It covers field visibility, sparse persistence, clearing, spoiler classes,
thread link targets and the unsaved CSS draft. The original auxiliary Settings
anchor is synthetic; its navigation affects focus timing. These records do not
qualify original-page focus or complete page placement. Native modal focus and
close behavior are checked separately.

The explicit-on reference initially failed because the owned shell omitted the
public client's `boardSelectMobile` dependency. Adding that static selector and
owned board-list containers allowed the unchanged client to initialize; no
client code or error assertion was suppressed. The earlier 24 editor, 50
shortcut, 16 refresh and 12 style records remain unchanged.

## CSS and storage boundaries

The public client inserts unrestricted saved CSS. The rewrite accepts a finite
catalog presentation language, using the existing [post CSS parser](native-custom-css.md)
with its catalog selector map. Accepted selectors are `.thread`, `.teaser`,
`.meta`, `.thumb`, `.txt-sub`, `.txt-rep` and `.txt-date`; the compiler scopes
them under `#threads`. Colors, fonts, spacing and the other presentation
properties use the parser's fixed value sets.

CSS is limited to 16,384 actual UTF-8 bytes, 64 rules, eight selectors per rule,
16 declarations per rule and 256 declarations total. Catalog theme JSON is
limited to 24,576 characters, and native settings to 4,096. Unknown catalog
fields, reserved keys, nonboolean flags and nonstring CSS are rejected.
Imports, resource URLs, functions, escapes, arbitrary selectors, positioning
and hiding declarations cannot be compiled. Unsupported stored CSS remains
inert text for review while admitted boolean options still work. An unsafe
draft cannot replace either stored key. Normalized rules enter an owned
constructed stylesheet that is removed on page suspension or owner detachment.

This intentionally limits custom styles that could load resources or cover
page controls. Users must adapt unsupported rules to the listed language.
Tests check actual colors, unsafe stored and edited drafts, unchanged storage,
no unexpected requests and stylesheet teardown. New-tab changes apply only to
canonical same-origin links for admitted thread IDs and add `noopener noreferrer`.

Persistent saves use the shared `paperboard-thread-watcher` Web Lock and compare
both exact values captured on opening. A competing change cancels a queued save;
a later attempt from that stale editor still cannot overwrite it. Close,
detachment and BFCache suspension cancel queued saves and spoiler changes before
lock entry. Event payloads and stored strings never become HTML or JavaScript.

Web Storage has no multi-key transaction. If a write fails, recovery restores
only values that still equal this save's writes. It preserves a replacement
from a nonparticipating writer and reports incomplete recovery. If recovery is
complete, or reads/locks/writes are denied, reviewed preferences stay in the
current tab with an explicit message; neither key is persisted without the
required lock. The native settings adapter also enters tab-only mode before
applying that fallback. The GET spoiler control and Reset change only
`nospoiler`, preserving other theme fields through the same lock.

The fixed `/static/catalog-theme.v1.js` route is admitted only on catalog pages.
It adds no worker, media, provider, import or network origin. GET/HEAD return
release-owned code with the existing MIME/cache/nosniff policy; writes, aliases
and API-listener requests are denied. Existing catalog worker and script CSP
tests retain healthy positive controls.

## Qualification

Before the final mobile wrapping adjustment, the Windows run passed all 152
selected production-template cases: Settings,
Search, filter lifecycle, shortcuts, spoilers and watcher presentation/icons.
The PostgreSQL-backed run passes all 38 Settings, navigation, transfer, CSS,
catalog preference and worker/CSP cases. The new navigation case reads the real
board directory and removes the actual bar without reloading. Its board-list
editor uses the existing bounded native consumer rather than an inert control.
All seven catalog-theme core and seven existing post-CSS cases pass. Fixed-asset
tests pass all nine cases; public all-target/all-feature Clippy and formatting
pass on Windows. Linux passed the same 152 template cases before the final
mobile wrapping adjustment, plus 93 public library
tests, nine asset cases, both seven-case CSS/core suites and public
all-target/all-feature Clippy. All five generated bundle checks pass. The
Windows YAML parser was unavailable; the installed Linux parser accepts the
workflow and verifies that every Windows step contains a command or action. The Linux
qualification launcher first refused an unwritable copied file; its later
core run stopped at a nonexistent npm script name after the listed tests
passed. The two remaining generated checks passed after correcting that name.

Failures were corrected against the recorded behavior and existing assertions;
available expanded-run traces and images remain in ignored local artifacts:
the initial 60-case run passed 50 and caught a zero-size close button, visible
desktop rows on mobile and font-family declarations that differed from the
record. Its 14 Settings cases then passed. An expanded 37-case run passed 34;
two fixtures clicked a hidden original Settings link after navigation mounted,
and one expected the wrong rejection message. The 152-case run next passed
139, exposing the unknown-storage default and a watcher test intercepting the
board-directory request. A narrower catalog-only matcher then missed actual
thread refreshes, producing 140 passes and 12 failures. The corrected matcher
retains all busy/disabled/icon assertions. One premature restart refused the
still-active listener; server reuse remains disabled. The final run passes all
152 without changing retries, timeouts, pixel tolerance or screenshot baselines.

The broader Linux database run initially passed 66 of 68 cases. Diagnostics
identified padding on an inline mobile catalog button extending the document
four pixels past the viewport. Mobile button wrappers now count that padding
during wrapping, with an inline-block and normal line height. This is a local
containment adjustment, not a newly measured original-page placement rule.
The other case assumed that a tab-only save closed its editor; it now checks
the explicit message and closes it before dragging. Both focused Windows
cases pass, followed by all 56 Search-style and Settings-lifecycle cases,
including actual stylesheet removal on suspension/detachment. The unchanged
24 control-style records still match. The broader Linux rerun passes all 68
database cases, including watcher position, actual pointer-capture races,
shared-lock lifecycle and native display/navigation. There are no ignored Rust
or core cases. These client tests add no new media-containment claim.

The successful commands include:

```text
node scripts/verify-public-catalog-settings-reference.mjs .local/references
node --test tests/browser/catalog-theme.test.mjs tests/browser/native-custom-css.test.mjs
cargo test -p board-public --lib --locked
cargo test -p board-public --test ui_assets --locked
cargo clippy -p board-public --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
npm run check:generated
npx playwright test --config playwright.themes.config.js tests/themes/catalog-settings.spec.js tests/themes/catalog-settings-lifecycle.spec.js tests/themes/catalog-controls.spec.js tests/themes/catalog-filter-lifecycle.spec.js tests/themes/catalog-shortcuts.spec.js tests/themes/catalog-spoilers.spec.js tests/themes/thread-watcher.spec.js tests/themes/watcher-icons.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/catalog-controls.spec.js tests/themes/catalog-settings-lifecycle.spec.js
npx playwright test tests/browser/watcher-settings.spec.js tests/browser/native-settings-transfer.spec.js tests/browser/native-custom-css.spec.js tests/browser/native-navigation.spec.js tests/browser/catalog-preferences.spec.js tests/browser/catalog-preference-locks.spec.js tests/browser/catalog-inplace.spec.js tests/browser/catalog-filter-worker.spec.js tests/browser/watcher-position.spec.js tests/browser/watcher-position-races.spec.js tests/browser/watcher-locks.spec.js tests/browser/watcher-lifecycle.spec.js tests/browser/native-display.spec.js
```

The eight-file production browser subset omitting the last five files passes
38 on Windows. The final 13-file command is the 68-case Linux run. Generated
checks used the individual package commands on Linux; the combined generated
command had passed in the preceding catalog slices with the same sources.

Full original-page pixels, complete navigation chrome and server teaser
normalization remain unfinished. The navigation component's existing local
layout is not newly qualified as original catalog pixels by the Settings record.
The catalog editor has the public catalog controls; Export remains available
from board/thread Settings, where its canonical board URL and transfer review
are tested. `catalog-theme` is outside the public v1191 export payload and is
not added to transfer. [Catalog filter transfer](catalog-filter-transfer.md)
now integrates its separate format into export/review and the shared restore
transaction. Hosted
checks are required before merging the combined catalog batch.

## Catalog readiness after navigation changes

The Linux browser jobs on the first two PR #188 heads each passed 31 of 32
quote-preview cases. The catalog exclusion case stopped at its readiness check:
it selected only the original Settings links, which the admitted desktop
drop-down navigation hides. The visible Settings button was present. The
backlink suite had the same catalog readiness assumption, although those jobs
stopped before reaching it.

Both suites now share the Settings opener already used to open the real dialog.
It admits the original visible links, the persistent-navigation button and the
custom-navigation link. Public defaults and feature assertions are unchanged.
In particular, the catalog cases still insert explicit quote-shaped fixtures,
require no preview/backlink graph or quote requests, and then navigate to a real
owned thread to check that the feature works there. All 82 quote-preview and
backlink cases pass on Windows and Linux through real PostgreSQL-backed posting
handlers. The Linux browser command passed all 82 in 6.8 minutes; its local
wrapper then failed on a trailing partial command because the wrapper file had
been edited while Bash was executing it. The wrapper's syntax check passes,
and subsequent qualification uses an immutable copy. No application or test
assertion changed to address that launcher error.

```text
npx playwright test tests/browser/native-quote-preview.spec.js tests/browser/native-backlinks.spec.js
```

## Initial catalog flags without Settings

The third PR #188 Windows job, 110212114460, passed all 68 media cases and
590 of 591 theme cases. The unsafe-stored-CSS case found the saved spoiler
flag applied but the catalog link's new-tab target absent. The retained image
also lacked the Settings links. That image does not establish why Settings
failed to mount, and the previous test did not retain startup diagnostics.

An owned reproduction aborts only the watcher script on image and text
catalogs. Both cases failed on the preceding implementation: the catalog read
the saved spoiler flag itself, while new-tab links depended on a Settings
event. The initial catalog read now applies admitted new-tab flags directly
to its existing canonical thread links. It retains `noopener noreferrer`,
performs no additional render and does not write storage. Settings events
still apply later changes.

The two cases require the deliberately absent Settings entry point, the exact
failed script path, every card's target and relationship attributes, the
spoiler flag, inert rejected CSS and unchanged stored preferences. After an
explicit new navigation with the script restored, they require the real
Settings controls and rejected-CSS review message. The lifecycle suite now
retains the existing bounded synthetic startup diagnostics on failure. This
fixes the reproduced initial-flag dependency; the original hosted startup
cause remains unproved.

The first combined local run passed 51 of 52 cases. Both initial-flag checks
passed; the text recovery step failed because its existing visual-only
`text-catalog` fixture has a hyphenated slug, which the native watcher correctly
rejects. The recovery case now uses the same text data at an additional owned
`settingstext` route with an admitted board slug. Existing text screenshots and
their fixture route remain unchanged; the production slug check is unchanged.

All 52 selected Windows cases pass, including the seven unchanged text-catalog
screenshot/live-row cases. Linux passes the 45 Settings lifecycle, Settings and
diagnostic cases, then fails the seven text screenshot cases because no Linux
captures are stored. That combined command exits 1 before its Rust checks; the
generated local captures are not accepted or committed. The text recovery case
itself passes on both platforms. Linux qualification of these flag changes uses
the 45 cases with stored reference/property assertions; Windows retains the
separate text screenshot checks.

```text
npx playwright test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js tests/themes/catalog-settings.spec.js tests/themes/text-catalog.spec.js tests/themes/visual-diagnostics.spec.js
```

The final immutable Linux launcher passes all 45 Settings/diagnostic cases,
all 16 native page-filter cases, all 70 catalog-filter cases and all nine public
asset/CSP cases, then exits 0 after formatting. Windows also passes its final
nine asset/CSP cases and public all-target/all-feature Clippy. The Linux
Settings command is:

```text
npx playwright test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js tests/themes/catalog-settings.spec.js tests/themes/visual-diagnostics.spec.js
```

## Relative links in inert cards

PR #188's fourth head passed the complete Windows visual check, monitoring and
media/operations qualification. The Linux browser job, 110222732882, failed
three catalog Search integration cases and passed 55 others in that group.
Their retained traces identify the same `Invalid URL` exception in the new
initial theme-link update. The unchanged behavior/live-search files reproduce
all three failures on Windows, with 19 of 22 cases passing.

The initial update used a thumbnail's resolved `href` without a URL base.
Cards inside an inert template belong to a document with `about:blank` as its
base, so their relative links are still relative. The shared-operator and
image-admission fixtures also contain links without `href` attributes. That
exception stopped Search initialization before its event handlers were ready.

The link update now reads the literal attribute, skips missing values and
resolves against the real page URL inside a guarded parse. Same-origin,
canonical board/thread, query and fragment checks still decide which links may
change. Invalid or foreign links keep their attributes. Four owned image/text
cases check the inert document base, admitted targets and relationship flags,
missing and malformed/foreign links, healthy Search and unchanged storage.
The original integration cases retain their sorting, debounce, shared grammar,
no-navigation and inert-image request assertions.

Windows now passes all 22 original behavior/live-search cases and all 22
Settings lifecycle cases. The first four new fixture assertions incorrectly
looked for inert cards through ordinary page locators; those cards intentionally
remain inside the template until Search is applied. The cases now inspect the
inert attributes first, then use the real Search control and require the same
card to become visible with its admitted attributes. No application change was
made to expose a card early for those assertions.

The final immutable Linux launcher passes the same 22 original integration
cases and 22 lifecycle cases, including all four new inert-link cases. Both
platforms pass all nine public asset/CSP cases. The Linux launcher then passes
formatting and exits 0. The focused browser commands are:

```text
npx playwright test --config playwright.config.js tests/browser/behavior.spec.js tests/browser/catalog-live-search.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js
```
