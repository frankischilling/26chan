# Settings and keyboard-help panels

Settings and Keyboard Shortcuts use the pinned source's panel geometry and
existing local theme icons. Keyboard help closes when its backdrop is clicked;
clicks inside the help panel leave it open. Close and Escape return focus to
its opener without saving or changing the underlying Settings draft.

## Source and scope

The reference is `js/extension.js` at
`545b7812d1849f7958d914950c91fdbbe38f6b22`:

- `8304–8355`: keyboard-help markup, backdrop command and close handler
- `9099–9117,9191–9213`: category icons and expanded/collapsed state
- `9646–9702`: theme families and 1×/2× icons
- `10503–10567`: centered UIPanels and the separate top-aligned Settings rule
- `10577–10589,11001–11015`: category icons and sub-option markers/spacing
- `10571–10573`: Tomorrow Settings-list borders
- `11063,11157–11160`: 320px panels at viewport widths up to 480px

The source CSS excerpt and full source-file hash are recorded in
`tests/fixtures/native-settings-panel-source.json`. The browser comparison
renders these literal rules in a separate script-free document. It compares
placement and width at the actual panel height, so revised help text does not
silently become a geometry oracle. This comparison does not establish equal
natural content heights, text wrapping or full-dialog pixels.

Settings stays 25px from the top with an 85% height limit. Keyboard help is
vertically centered with a 100% height limit. Both use 400px desktop and 320px
narrow-viewport widths. The existing viewport-minus-20px width ceiling remains
for screens narrower than the source panel; content remains scrollable.

The close and disclosure controls use the existing `static/watcher` PNGs.
`docs/public-watcher-assets.json` records their hashes and dimensions. Theme
styles choose fixed local URLs, including 2× assets at 2dppx. Accessible buttons,
labels, disclosure state and keyboard focus remain in place. No external asset
fetches or runtime URL construction are introduced.

These rules are scoped to Settings and keyboard help. Catalog, filter help,
export and Custom CSS keep their separate layouts. Config/default persistence,
MainInit/ParsingDone, report and deletion behavior are unchanged. Playback and
transport compatibility flags remain inactive. This is a bounded panel
restoration; it does not establish complete Settings or full-page parity.

## Checks

The category fixture now serves production board, theme and mobile CSS instead
of unstyled controls. It preserves the existing category, draft, visibility,
initialization, cancellation and save cases. Twenty added browser cases cover
six themes at 1×/2×, desktop/mobile source geometry, image requests, sub-options,
Tomorrow borders, inner/backdrop clicks, Close/Escape, focus return and a
280×220 viewport. Additional cases start on mobile with the actual mobile
Settings matrix, change theme while both panels stay open, and click Close
after scrolling short panels. Mobile startup never invents a keyboard-help
entry that the source hides. Tests use synthetic local pages and fixed local
asset routes; no database or live provider is needed.

Fresh checks on the reconstructed slice based on
`73bc2e2b9b9734606a8278ce68cdab3921b54305` passed:

- `npm run test:settings-visibility-core`: 23 cases, including asset hashes,
  source extraction and keyboard-help event handlers
- `node --test apps/public/client/native-keybinds.test.js`: 4 cases
- `npm run test:watcher-core`: 192 cases across its 133-, 18- and 41-case groups,
  with no failures or skips
- `npm run check:generated`: all generated assets match their sources
- `node scripts/record-native-settings-panels.mjs --check --source ../26chan-reference/js/extension.js`:
  full pinned-source and derived-fixture verification

The native filter bundle was rebuilt because it contains keyboard help.
Playwright discovered all 52 category/panel tests. A selected browser run
stopped before executing its assertions because the configured Chromium
headless-shell executable is absent. Geometry, image loading and native focus
assertions therefore remain unqualified locally. These results come from the
reconstructed files; earlier workspace results are not reused as evidence.
Independent review of the reconstructed implementation found no blocking
defect and reproduced the 23+4 focused checks and source verification. An
exact-head browser run remains required.

## Reproducing the source fixture

`node scripts/record-native-settings-panels.mjs --check` verifies the retained
raw source snippets against pinned per-snippet hashes, derives the CSS excerpt
and all twelve keyboard-help rows, then compares the complete generated fixture.
The Node suite also rejects modified snippets and compares rendered help-row
text against these source-derived rows, including the original em dashes.

With the pinned source checkout available, add
`--source ../26chan-reference/js/extension.js`. This verifies the full source
file hash and checks the retained snippets against fresh extraction. Omitting
`--check` regenerates the derived fixture. Neither path executes the old script.

The source's DPR threshold remains at least 2; this change does not alter it.
