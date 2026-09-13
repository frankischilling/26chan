# Public theme preferences

Work in progress for [issue #52](https://github.com/frankischilling/26chan/issues/52).
The public reference exposes six named styles: Yotsuba, Yotsuba B, Futaba,
Burichan, Tomorrow and Photon. [Asset provenance](public-theme-reference.json)
records the actual versioned URLs, byte counts, collection timestamps and hashes.
The collector retained public static stylesheets and the publicly served UI
script, not user posts or uploads. The two fixed UI gradient PNGs are versioned
local assets with recorded source hashes.
The original implementation checkout remains unused.

The stylesheets' `body`, ordinary link/hover, reply, subject, name, greentext,
quote-link, posting-label and target selectors supply the implemented palette
and base-font choices. Yotsuba, Yotsuba B, Photon and Tomorrow specify Arial at
10pt; Futaba and Burichan specify Times New Roman at 12pt. The observed work-safe
board selects Yotsuba B. The public FAQ also documents blue defaults for
work-safe boards. This evidence is about public presentation, not posting or
private moderation internals.

The public client's `initStyleSheet`, `setActiveStyleSheet` and
`getPreferredStyleSheet` read/write a cookie per `style_group` for 365 days and
choose Yotsuba B for `ws_style`. The observed board declares that group.

The local implementation applies Yotsuba B by default to work-safe board,
thread, catalog, archive and upload views, and Yotsuba otherwise. A finite
explicit browser choice overrides that default independently for work-safe and
other boards. The same-origin preference
page and form work without JavaScript and return only to allowlisted local
read-route shapes. Referrer query strings are discarded. Processing/status
POST paths are not used as return destinations.

The preference cookie has no authentication or media authority. Production uses
`__Host-board-theme` and `__Host-board-theme-ws` with Secure, HttpOnly, Path=/,
SameSite=Lax and a one-year maximum age; development omits the `__Host-` prefix
and Secure for loopback HTTP.
Neither sets Domain. Invalid, oversized or duplicate preferences fall back to
the board default. Unknown selections cannot supply CSS or URLs. Theme updates
retain public admission, write-rate, timeout and origin checks and have a
1024-byte form limit. No database write or new runtime credential is involved.

The shared base CSS remains public. Theme CSS and preference-page responses use
`private, no-store` and `Vary: Cookie`; conditional requests do not turn one
selection into a 304 for another. CSP permits the two exact local gradient
paths as image sources, plus the configured separate media origin when enabled.
It does not permit all same-origin images. Script restrictions remain unchanged,
and theme styles load no remote resources. The URL crate declaration
reuses the locked workspace version for safe referrer-origin parsing.

The reference's parent-domain, JavaScript-readable cookies and immediate
JavaScript selector are replaced by host-only HttpOnly cookies and a server
form. This preserves the script-free public CSP and prevents preference cookies
from crossing into staff/media domains. Users open Style and apply a selection;
the extra navigation is a documented security-driven UI difference, not exact
control-layout parity.

## Current verification and remaining work

These local commands passed on Windows:

```text
cargo test -p board-public --lib --test themes --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo build -p board-public --example visual-fixtures --locked --jobs 1
npx playwright test --config playwright.themes.config.js
npm run test:behavior
python scripts/check-media-parser-dependencies.py
cargo fmt --all -- --check
```

The final public suite passed 51 tests against the owned disposable PostgreSQL
cluster, including the repeated upload/post/delete browser regression. All eight
real-server browser behavior scenarios passed. The parser dependency guard
passed its allowed-guest and injected-edge rejection controls. Clippy passed with
warnings denied. These Windows results do not replace native Linux or production
containment qualification.

The five focused HTTP tests use the actual public router with an unavailable database
and verify cookie flags, private CSS, GET/HEAD behavior, origin denials, malformed
input, body limits, independent cookie groups and safe return navigation. Bounded
property tests exercise arbitrary return paths and cookie values. The two Chromium scenarios apply
all six styles at desktop/mobile sizes with JavaScript disabled, assert computed
colors/fonts and unchanged synthetic post text, reload the selected style, check
the preference form and cookie scope, verify group independence, and detect
horizontal overflow. Twelve Windows screenshot comparisons cover every style
at both viewports. Each final image was inspected for readable fields, font and
palette selection, and mobile wrapping. Tomorrow's field borders/placeholder and
Photon's label borders were corrected against the pinned CSS before final review.

The real application browser CSP test passed with
`npx playwright test tests/browser/behavior.spec.js --grep 'theme backgrounds'`.
An exact allowed gradient loaded at 1 by 200 pixels. The same healthy PNG endpoint
on an alternate hostname returned matching bytes to the HTTP client but failed
to load in the protected page with an `img-src` violation. Its first assertion
expected an origin-only violation report; the trace showed Chromium reports the
full URL. The assertion now checks that exact URL without relaxing the policy or
positive/negative load controls.

The existing 17 comparisons were first run without updates using dedicated
`.local/theme-review/{board,archive,media}` output directories. All reported
screenshot differences from the intended font, label palette and footer changes;
the preceding behavior and image-dimension/non-fetch assertions passed. All 17
actual captures were individually inspected, then only those three suites were
regenerated. SHA-256 comparisons proved every new baseline equals its inspected
capture. No browser pins or pixel tolerances changed. The 12 new theme images
also pass ordinary comparison runs with JavaScript disabled.

The final ordinary run passed all 29 screenshot comparisons in 19 scenarios:

```powershell
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:themes
```

[PR #53](https://github.com/frankischilling/26chan/pull/53) merged as
`8ceffb7ca375cc07f6a94348e00752676c4ddc49` after reviewed head `9ad6d2a`
passed complete [PR](https://github.com/frankischilling/26chan/actions/runs/34739165013)
and [push](https://github.com/frankischilling/26chan/actions/runs/34739163311)
Linux/Windows, advisory and monitoring checks. The main merge-preview tree
matched the tested head. The Windows CI job includes
`npm run test:themes`; the real CSP test runs in the existing Linux behavior step.
Full original layout, spacing, responsive
behavior, additional UI states and the separate architectural brief remain
unverified under #6. Arial and Times New Roman file hashes and the pinned browser,
Windows environment and viewports are in the main reference manifest. These are
local regression baselines, not original-site or all-platform parity evidence.
Production qualification remains under #5.
