# Attachment visual regression

Eight new project-owned PNG baselines cover board, thread, catalog and archived-thread layouts at 1280×900 and 390×844. They use the production Askama views and unchanged production CSS. Each page contains landscape, portrait, small, legacy full-image, spoiler and deleted attachments, with fixed timestamps, synthetic comments and an escaped long filename. No original-site screenshot, user upload or unverified source checkout supplied these fixtures.

The fixture constructs blue/gold geometric pixels in the bounded RGBA protocol, then uses the normal validator, full/thumbnail encoder, publication store and hash-checked reads. It serves the resulting bytes on a second loopback listener, `http://localhost:3004`, while HTML uses `http://127.0.0.1:3000`. It has no database, posting or approval endpoint. This fixture is a layout test, not media-reader authorization, dispatcher authentication or worker-containment evidence; those properties have separate [attachment](post-attachments.md) and [native dispatch](verification-media-dispatch.md) tests.

## Pinned environment

The local run used Windows, Playwright 1.62.0 and its Chromium 151.0.7922.34 (revision 1234), with device scale 1, en-US locale, America/New_York timezone, light color scheme and JavaScript disabled. The production font stack uses Windows Arial/Tahoma with Helvetica/sans-serif fallbacks. CI runs the committed Windows baselines on `windows-2025`; this does not establish other operating-system or font parity. Baseline filenames retain the platform suffix. No dependencies or browser pins changed.

Local font provenance, read with `Get-FileHash -Algorithm SHA256`: `C:\Windows\Fonts\arial.ttf` was `BAA251526D6862712A58E613EF451D8A2B60482142EC6AAB1D47FB8E23E21A7C`; `C:\Windows\Fonts\tahoma.ttf` was `F936530A7EDE296580F897C47E7A3FA48A9483166080CE05105673A1339CBF0C`. These identify the baseline environment; they do not assert that every hosted runner image has identical font files. Investigate a runner/font change before accepting screenshot differences.

The tests wait for all four visible files to load. They require exact rendered landscape/portrait/small dimensions of 250×150, 100×250 and 48×32, or 150×90, 60×150 and 48×32 in the catalog. The legacy full-image preview uses its full numeric PNG path and remains bounded. Modern previews use the numeric thumbnail path. Spoiler and deleted IDs must never be requested, including after opening spoiler details. Other assertions check escaped filenames, isolated link attributes, deleted placeholders, absent archived posting forms and no horizontal overflow.

## Commands and results

```text
cargo clippy -p board-public --example visual-fixtures --locked --jobs 1 -- -D warnings
npm run test:media-visual -- --update-snapshots
npm run test:media-visual
```

Clippy passed with warnings denied. Initial generation created only the eight new media snapshots. All eight were visually inspected for filename wrapping, readable forms, image proportions, catalog containment, spoiler links and deletion state. The ordinary rerun passed all eight with zero differing pixels. No production layout changes or existing baseline updates were needed.

Existing baseline checks also passed against the expanded fixture server:

```powershell
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:visual
npm run test:archive-visual
```

All three text-board/catalog and six archive tests passed without updates. The CI change adds `npm run test:media-visual` after these existing Windows checks; it does not alter workflow permissions, runners, dependency pins or native qualification. All 17 comparisons passed [PR CI on `9a8e5b1`](https://github.com/frankischilling/26chan/actions/runs/34735844313). Original-site visual parity, additional themes and broader loading/error-state coverage remain tracked separately in [compatibility](compatibility.md) and issue #6.

The JPEG input slice changes the upload label to "PNG or JPEG". Only its four board/thread desktop/mobile baselines were updated with `npm run test:media-visual -- --grep 'attachment (board|thread)' --update-snapshots`. Each was inspected for readable forms, wrapped filenames, image proportions and mobile overflow. All 17 comparisons then passed locally without updates. No CSS, catalog/archive baseline or browser pin changed. New-head CI remains required; see [JPEG verification](jpeg-media.md).

The subsequent [theme slice](public-themes.md) updates the referenced base font,
palette and footer style control. Its record covers the deliberate comparison,
individual review and update of all 17 existing baselines. Media dimensions,
spoiler/deleted non-fetch checks and browser pins remain unchanged. Theme PR CI
is separate from the earlier results above.
