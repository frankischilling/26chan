# Catalog display preferences

The pinned public `catalog.min.1025.js` reads `localStorage["catalog-settings"]`
and saves `orderby`, `large` and `extended`. Its change handlers persist sort,
image size and teaser changes. The source hash and inspected functions are
recorded in `public-catalog-reference.json`; no public post data is required.

The local catalog now saves the same three finite fields on control changes or
Apply. A fresh catalog visit restores them through the existing validated query
route. Storage is origin-local and is not a cookie, authentication state or a
server-side preference. Search text is not persisted. An explicit URL containing
any display option takes precedence without overwriting the saved preference.
Reset clears the key and navigates to explicit defaults with an empty search.

This closes the display-preference persistence gap, not complete catalog parity.
The reference redraws in place; this implementation still makes a GET navigation
on changes and may need an additional GET on restoration. That difference remains
unfinished interaction work, not an asserted security requirement. Explicit URL
precedence and Reset's URL behavior are local extensions. Quick-search persistence,
filters, hidden/pinned threads and the remaining catalog interactions are outside
this change. Without JavaScript, the existing Apply and Reset form still works;
browser-local preference restoration is unavailable.

## Browser safety

Only successful catalog HTML responses allow the exact release-owned
`/static/catalog-preferences.v1.js` URL in CSP. Other public responses retain
`script-src 'none'`. Inline script, event attributes, arbitrary same-origin
scripts and third-party scripts are not enabled. The script is compiled into the
application with literal GET/HEAD routing, JavaScript MIME, `nosniff`, and
revalidation-required caching. It is not exposed through an upload or filesystem
route and is not added to the image source allowlist.

Stored input is limited to 1024 UTF-16 code units before JSON parsing. The script
accepts only the four supported sort strings and two actual booleans, ignores
unknown fields and never merges stored objects. It does not use stored values as
HTML, executable code, paths or origins. It builds a URL from the current page
and supplies only allowlisted display values. Missing, malformed, oversized or
unavailable storage falls back to the rendered page. Storage failures do not
prevent GET form submission, and Reset's explicit defaults avoid a restoration
loop when removal is denied.

There are no database migrations, new dependencies, staff changes or media-policy
changes. Production media remains disabled and production qualification is not
established by this feature.

## Checks

`apps/public/tests/ui_assets.rs` checks exact script bytes, MIME, caching,
GET/HEAD, denied writes, unknown paths and exclusion from image CSP. The existing
image-route tests remain unchanged.

`tests/browser/catalog-preferences.spec.js` exercises actual-server restoration
in a new tab, changes, reload, Reset, explicit query precedence, search privacy,
malformed and oversized inputs, ignored extra fields and denied storage. It also
checks the actual catalog CSP with a healthy alternate-script control, blocked
inline execution, the working allowed script and unchanged script denial on
non-catalog and invalid-query responses.

The existing no-JavaScript persisted posting/catalog browser workflow and pinned
theme/visual suites remain regression gates. No screenshot baseline needs to
change for preference persistence. Run `npm run test:behavior`, the standard
visual suites and the public Rust tests against the repository's owned fixture
and database environments. Passing these checks does not establish complete
reference interaction parity or production readiness.
