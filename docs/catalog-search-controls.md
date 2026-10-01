# Catalog Search controls

Search now opens the compact field used by the pinned public catalog client.
Clicking Search again, clicking × or pressing Escape closes it and restores the
catalog. Closing retains the old field value; reopening clears that value and
focuses the field. Unmodified S from the document body opens Search. When Search
is already open, S clears its field and labels while retaining the existing
filter until the next keyup. Ctrl, Alt and Shift combinations do not open it.

Filtering follows keyup after 250 ms. Enter uses the same debounce and does not
submit a document request. An input event alone does not filter. The released
client does not wait for composition to finish when a keyup arrives; the local
controls now follow that behavior. Closing does not cancel a pending debounce.
That pending search can run after the field has closed. These observable quirks
are covered explicitly rather than replaced with different local behavior.

The top and bottom Search labels use text only. The bottom label stays hidden on
mobile, matching the released stylesheet. The field is 75 CSS pixels wide;
modern and classic themes retain their separate font, padding and border rules.
Focus border colors and mobile control backgrounds follow all six public themes.
The existing light/blue fixed gradients are reused. Tomorrow adds the unchanged
105-byte public dark gradient as one literal reviewed image route.

## Reference and admission

[The reference record](public-catalog-ui-reference.json) contains 46 behavior
states and 24 independent style cases. Its
[replay script](../scripts/verify-public-catalog-ui-reference.mjs) executes the
unchanged hash-pinned public catalog client v1025 on two owned synthetic cards.
It checks the six hash-pinned public catalog stylesheets and mobile CSS v705,
denies nonfixture requests and checks for page errors after each interaction.
The browser is Chromium 151.0.7922.34 at density 1 and widths 1280/390. Both the
reference and local comparison pause the browser clock before timer assertions.
Focus is recorded only while the field is shown: initial replay showed that
hidden-input blur timing depends on when the browser completes layout.

The September 30 DOM observation retained only static selector shape. No post
text, embedded catalog data, filenames, field values or production media were
retained. The cards and their values are owned fixtures. This establishes the
listed controls, not complete original-page parity or unknown server-side teaser
normalization. [The asset record](public-catalog-control-assets.json) gives the
public URLs, collection times, exact bytes, hashes and fixed gradient dimensions.

The local script enables the controls only after the catalog's complete bounded
metadata and current-page form target pass admission. It binds original local
buttons, keeps the released DOM identifiers and uses lexical event handlers.
It does not execute callback names or stored HTML. The same escaped server cards
and inert partition still supply all filtering. The 128-scalar query limit,
storage bounds, matching grammar and media admission remain enforced.

Without JavaScript or complete metadata, the visible GET form retains its Search
field and Apply button. Explicit `q` parameters, canonical local URL updates and
GET fallback remain documented local interface extensions. The older synthetic
fixtures without the new controls retain their input/composition enhancement;
they qualify that fallback rather than the released Search interaction.
Same-board sessions and bounded `#s=` fragments open Search with their restored
query. A board change clears both session keys. Closing or applying an empty
search removes the saved query while retaining the board key, as in the reference.

No database migration, staff authority, media processor or credential policy
changes. The new gradient uses the existing fixed-byte GET/HEAD route contract,
MIME and cache policy. CSP adds that exact image path and permits no upstream
asset origin, wildcard path or runtime download.

## Local verification

The independent reference replay passes all 46 states and 24 style cases.
`tests/themes/catalog-controls.spec.js` passes 40 tests against the actual
production templates and local script, including every recorded behavior and
style state. Its owned board identifier maps to the reference's synthetic board
identifier for session comparisons; card IDs remain exact.

The persisted catalog, fallback, thread-state and preference run passed 43 of 44
cases. The remaining assertion identified a fixed-image CSP ordering mismatch:
the new literal route was placed earlier than its manifest entry. The table now
follows the manifest without changing the assertion or allowed origins. A first
Rust asset run overlapped the Windows browser server and could not replace its
running executable. The sequential rerun passed all 89 public library and eight
fixed-asset tests, with zero ignored cases. Public all-target/all-feature Clippy
and formatting passed. The remaining focused rerun and theme qualification are
recorded below.

The exact persisted CSP rerun passed, including fixed-image dimensions and the
healthy denied-origin control. The full Windows theme run passed 440 of 444
cases in 6.9 minutes and exposed four catalog integration failures: delayed
native-button blur suppressed the body-only S shortcut, two paused-clock preview
tests still waited for immediate Apply, and an empty fallback wrapper introduced
a toolbar gap and an extra flex row. Closing now releases the owned control's
focus; the preview cases advance the unchanged 250 ms debounce; empty fallback
wrappers no longer alter the GET form layout.

The corrected build passed all 96 catalog/text-catalog theme cases in 1.4 minutes,
including every earlier failed case. Desktop/mobile limit captures passed
unchanged after inspection of both prior differences. No screenshot baseline,
pixel tolerance, retry or timeout changed. All five generated checks and eight
preference-lock core cases also passed. The fixed-asset reference replay passes
all six themes, both widths and densities 1/2. Hosted checks remain required
before this next integration merges.
