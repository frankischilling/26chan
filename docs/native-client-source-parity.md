# Native client source comparison

Issue: [#218](https://github.com/frankischilling/26chan/issues/218).
Reference: supplied revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
The extension, core and catalog files have SHA-256 values recorded in the
Settings and announcement fixtures. This comparison concerns the controls and
client lifecycle. Page artwork, server posting rules and unavailable account
services retain their own issue scope.

## Changes in this continuation

The WebM/MP4 player already consumed `unmuteWebm`, but users could not reach it
in Settings. Images & Media now exposes the source's audio control on desktop
and mobile. Export and reviewed restore retain it as an active boolean. It
defaults off; enabling it does not override browser autoplay permission.

[Global announcements](global-announcement.md) now have a bounded operator
publication path, escaped page content, source timestamp dismissal and separate
mobile behavior. The catalog control does not depend on the extension's
`disableAll` preference. This is separate from the existing blotter feed.

The complete `Config`, `ConfigMobile` and `SettingsMenu.options` data is retained
as pinned source snippets. The extractor accepts only a limited object/array,
string and boolean grammar; it does not execute the old application. Tests
compare every setting's default and desktop/mobile availability, including the
same-host HTTPS condition, against that source-derived matrix. Browser category
expectations come from the same extracted source, not a second manually copied
list of implemented controls.

## Extension modules

All 34 top-level modules in the supplied `js/extension.js` are accounted for
below. The implementation paths are under `apps/public/client/` or
`apps/public/static/`; test names are under `tests/browser/` unless stated.
A mapping identifies the relevant implementation and qualification surface;
it is not a claim that a filename alone proves equivalent behavior.

| Source modules | Current implementation | Principal checks |
| --- | --- | --- |
| `Parser` | Typed server projections, `native-post-tree`, `native-comment-projection`, `native-source-events` | `source-parsing-events.spec.js`, quote/updater projection suites |
| `OgvCtrl` | Pinned local OGV WebM fallback and bounded native player selection | `native-images-dom.test.mjs`, `check-ogv-assets.py` |
| `PostMenu` | `thread-watcher.v1.js` post-menu construction and ready event | `post-menu-events.spec.js`, media-visual post-menu suite |
| `Search` | Native index search and catalog search/filter controls | `catalog-search-contract.spec.js`, `catalog-live-search.spec.js` |
| `Depager` | `native-depager` and bounded transport | Depager browser suite and source-event checks |
| `QuoteInline`, `QuotePreview` | `native-inline-quotes`, `native-quote-preview` and finite projection recipes | Their DOM/browser suites, copied-post ownership and cancellation cases |
| `ImageExpansion`, `ImageHover` | `native-images`, approved media origins and local video fallback | Image/video DOM suite, owned upload/browser qualification |
| `QR` | `native-quick-reply`, drawing controller and bounded upload state machine | Quick Reply, upload, drawing and replay suites |
| `ThreadHiding`, `ReplyHiding` | Native hiding/state modules and scoped storage | Thread/reply hiding browser cases, filters and cross-tab races |
| `ThreadWatcher` | Watcher core, post tracking, auto-watch and preference locks | Watcher lifecycle, archive, automatic-watch, locks and settings suites |
| `ThreadExpansion`, `ThreadUpdater` | Native thread controls, coherent snapshot application and updater | Expansion, updater, tail and notification suites |
| `ThreadStats` | Persisted server counts and native statistics controller | Statistics HTTP/DOM/browser cases |
| `Filter` | Native editor, disposable matcher and page/catalog filter modules | Filter worker/settlement, editor/help, hide/highlight and catalog suites |
| `IDColor` | Native poster IDs, identity actions and source color rules | `native-poster-ids.test.mjs`, `native-poster-id-actions.test.mjs` |
| `SWFEmbed` | No SWF controls are advertised before an approved SWF backend exists | Backend dependency [#206](https://github.com/frankischilling/26chan/issues/206) |
| `Linkify`, `Media` | Native linkification and explicit provider embedding | Linkification and `native-embeds` suites, mobile and disabled controls |
| `StickyNav`, `CustomMenu`, `Draggable` | Native navigation, custom board list and bounded position adapter | Navigation, watcher-position and race suites |
| `CustomCSS` | Native editor and finite CSS grammar | `native-custom-css` unit/browser cases |
| `Keybinds` | Native keyboard controller and source help rows | `native-keybinds.spec.js`, Settings/help panel checks |
| `Del`, `Report` | Confirmed/uncertain deletion outcomes, scoped forms and report popup | Native deletion, report popup and actual persisted post-menu cases |
| `UA` | Explicit mobile-device/layout decisions at the relevant feature boundaries | Settings startup, layout, navigation, quote and media cases |
| `Config`, `ConfigMobile`, `SettingsMenu` | Native settings, shared locks and reviewed transfer | Complete source matrix, category, startup, HTTPS, transfer and cross-tab cases |
| `Feedback` | Escaped, owned feedback for media, deletion and drawing operations | Error, cancellation and retained-draft cases in those feature suites |
| `Main` | Watcher bootstrap, page chrome, source lifecycle events, navigation and announcements | MainInit/ParsingDone, settings-before-init, disabled extension and announcement suites |

## Core and catalog behavior

Core `TCaptcha` depends on the challenge/ticket service in #199. `PainterCore`
and replay controls use the completed drawing backend. Core tooltip, board flag,
mobile navigation, post-form, local preference, math and stylesheet controls
have their native counterparts. `StorageSync` behavior is provided through
scoped preference events and locks; global aliases are not used as authority.

The source mobile-device navigation defaults apply only on first run. Mobile
layout independently changes linkification, YouTube behavior and compact-thread
layout. Saving hidden preferences does not erase them. `disableAll` suppresses
extension features while core navigation, source forms, mobile announcement
controls and Settings remain usable. Settings startup, layout and relevant
feature tests cover these distinctions.

The separate catalog has its own settings, positions, search, preview and filter
contracts. MainInit is emitted after its settings and theme preparation; the
extension does not emit a duplicate catalog event. Catalog announcement
initialization preserves the source's WebStorage/mobile conditions.

The source's recursive-reply storage key has no reachable built-in menu action
in this snapshot; the restore call is commented out. Seasonal stylesheet
suppression is conditional on a configured event stylesheet, which is disabled
in the supplied configuration. Those findings do not authorize inventing UI
for unavailable server features.

## Boundaries and dependent work

- Pass, public email sign-in and CAPTCHA/tickets remain #201, #200 and #199.
  Their client controls must be connected when those services exist. The complete
  original one-submit posting flow remains #197.
- Random title-banner artwork and full page/theme capture remain #193/#6.
  The current empty banner container is not claimed as reproduced artwork.
- SWF remains #206. No inactive preference or empty player counts as playback.
- Scripts construct finite DOM nodes and escaped text. Custom CSS, imports,
  matching workers, media loads and responses keep their existing work/origin
  bounds. Unknown deletion outcomes remain visible and require a refresh;
  they are not falsely reported as source-success responses.
- The old `Main`/`Config` global objects are not published as a new scripting
  API. The supported source events have their own ordering tests. Private
  staff-tool consumers of old globals belong to staff workflow work.

## Qualification

Local checks currently pass the complete source Settings matrix, source
announcement vectors, focused visibility/startup/panel tests, transfer boolean
round-trips, formatting and generated-asset checks. Real PostgreSQL store tests
pass announcement publication, revision fencing, withdrawal, read-only grants
and readiness drift rejection. The populated announcement upgrade and
administrator restore also pass.

Application HTTP, browser, Windows visuals, the existing lifecycle/interaction
suites and every exact-final-head CI gate still require hosted qualification.
The local browser executable is unavailable; the source and database results
above are not a browser pass. This issue stays open until its complete candidate
has passed those checks and the remaining inventory review is resolved.
