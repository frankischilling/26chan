# Referenced posting fields

[Issue #60](https://github.com/frankischilling/26chan/issues/60) covers the
desktop posting-field geometry recorded in [the form reference](public-form-reference.json).
The six public v716 stylesheet hashes and the client-script hash come from
[the theme manifest](public-theme-reference.json). A read-only public `/po/`
response supplied structural tags and field attributes. No field values,
response body, production posts, user images or private source became fixtures.

The observed desktop form uses `table.postForm#postForm`, text inputs named
`name`, `email` and `sub`, a submit control in the subject row, and a four-row
`com` textarea. Its client `showPostForm` function sets `display: table`.
The synthetic reference applies that one style without running the original
script, captcha or external requests. A separate reply form was not observed.

`scripts/verify-public-form-reference.mjs` checks source byte counts and SHA-256,
the pinned Chromium version, and standards mode before comparing recorded
properties. It aborts every browser network request. The command requires the
separately retained public CSS directory and does not download content:

```powershell
node scripts/verify-public-form-reference.mjs .local/reference/themes-20260913
```

The actual application has six browser cases comparing form width, table
spacing, label padding/borders/font size, text field geometry, and textarea
dimensions/fonts against these facts. The cases also exercise editable
script-free mobile board and reply forms, visible focus outlines, rejection
of incomplete forms followed by valid filled forms, exactly one submit
control, and horizontal overflow. Full page geometry and original
posting behavior remain unresolved under #6.

## Implementation and differences

Normal and approved-image posting share an escaped Askama field macro. The
`postForm` ID now identifies its table, as observed publicly; an enclosing
`form.postEditor` keeps the actual POST action and hidden parent/receipt fields.
Explicit labels preserve accessible names. The table has `role="presentation"`
because it arranges controls rather than tabular data. Comment help is linked
with `aria-describedby` in both flows. Field names, validation limits, posting
options, required password, capability handling and upload authority are
unchanged. A new thread's submit control shares the subject row; replies, which
retain the project's subject omission, put it beside the name field.

The reference properties include 468px desktop table width, 1px spacing, 244px
text-field content width, and 292px by 60px textarea content dimensions. These
controls use the measured content-box sizing. Theme-specific label borders,
padding, textarea margins and serif-theme monospace comments are preserved.
At up to 600px, fields fill their available column with border-box sizing and
16px text so they remain usable without horizontal scrolling. Mobile adaptation,
the options selector, explicit deletion password and approved-image submit
wording are local behavior, not observations of the original form.

The script-free CSP keeps the form expanded; original client scripts and
captcha services are not introduced. The separate upload/status/approval flow
and explicit password remain the existing security choices. The rewrite does
not yet provide an equivalent captcha or claim equivalent production abuse
protection. Those differences are not hidden by the visual comparison.

## Verification history

The new application regression first failed because the form was 490px wide
instead of 468px. The first temporary measurement omitted a doctype and used
quirks mode; the separate reproduction command caught its incorrect box-sizing
fact. The public page declares `<!DOCTYPE html>`. Re-measuring standards-mode
synthetic markup produced content-box fields and a 60px textarea content height;
the application and committed facts were corrected to match. All six source
reproductions and desktop application cases then passed.

The added mobile validity check initially used an unscoped deletion-password
label and matched both the posting field and existing post-deletion fields.
It now selects the posting form explicitly. Actual normal-server browsing,
posting, reporting, deletion, options, Unicode and origin/CORS scenarios passed
all nine cases. The public all-feature suite passed all 52 tests and clippy
passed with warnings denied. That suite includes actual approved-attachment
posting through restricted public/intake/reader handlers; its local supervisor
supplies synthetic bounded pixels, not a claim of local VM decoding.

All 20 changed regression captures were individually inspected: two boards,
four attachment board/thread views, two empty boards and twelve theme views.
A mobile media capture exposed a filename under the leftover mouse pointer
after scrolling. Moving the pointer to the page corner before capture removes
that incidental hover; the corrected image was inspected and the other three
affected media captures were byte-identical. Actual approved-image form
screenshots were also inspected at desktop and mobile widths, and its browser
flow now always checks both viewports before posting. These are synthetic
application captures, not original-site snapshots.

Final ordinary visual runs passed all 41 scenarios and 39 screenshot
comparisons: board 3, archive 6, attachment 8, states 10 and themes 14. The
theme suite comprises six form cases, six post-layout cases and two preference
scenarios with twelve screenshots. All 20 changed baselines matched the
reviewed captures byte-for-byte; the other 19 remained unchanged. Both local
reference reproduction commands passed for all six themes. Formatting,
JavaScript syntax, whitespace and the media parser dependency guard passed.

```powershell
cargo test -p board-public --all-features --locked --jobs 1
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
npm run test:behavior
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:states
npm run test:themes
node scripts/verify-public-form-reference.mjs .local/reference/themes-20260913
node scripts/verify-public-post-reference.mjs .local/reference/themes-20260913
cargo fmt --all -- --check
python scripts/check-media-parser-dependencies.py
```

PR #61 merged as `96ea7001288698822f9559a5d3cabf0d92b6d62d` at
2026-09-13T06:35:07Z after final source review and matching the merge-preview
tree to tested head `d216b62aa51469a3466c2c4ec1d7560959328e72`.
PR build 34742232569 passed Linux qualification (17m49s) and Windows visuals
(4m35s); push build 34742229924 passed both (20m4s and 4m23s).
Monitoring runs 34742232501 and 34742229909 passed. All native guest, dispatch,
intake, restore, pressure and maintenance-alert steps completed successfully.
The advisory workflow was path-filtered with no dependency changes. No
dependency, migration, credential, workflow permission, production enablement,
deployment or release is included. Browser pins, zero retries and zero-pixel
tolerance are unchanged.
