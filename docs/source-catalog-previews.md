# Catalog hover previews

Issue #163 restores the ordinary-author hover preview from the supplied source.
The reference was inspected on 2026-09-14: `catalog.php:50-110,156-168`,
`js/catalog.js:2154-2350`, and the six catalog styles' `#post-preview` rules.
Their eight SHA-256 hashes match the [text-catalog source manifest](source-text-catalog.md).
The source was read, not executed. Captures contain synthetic rewrite fixtures.

## Behavior

Hovering a catalog thumbnail, including the no-file/deleted placeholders, waits
250 ms before showing `#post-preview`. On text-only boards the date cell is the
hover target. Mouseout cancels the timer and removes the preview; sorting,
search, hiding and opening a thread menu also discard it. Ordinary subject
links do not trigger it. Text-board dates remain hidden at 480px and below.

The header shows the subject on image boards, or `Posted` when the subject is
empty or the board is text only, followed by the author, relative age and page.
Page numbers use the snapshot's sticky/bump order and configured page size,
independent of display sort, pins and filtering. The prepared catalog teaser is
included when image-card teasers are hidden, and always on text-only boards.
It uses the same saved formatter and current teaser policy as display/search;
the subject is not repeated in the teaser paragraph.

The latest visible reply contributes its author and relative age. Deleting it
selects the preceding visible reply; deleting all replies removes the last-reply
line. Current forced-anonymous policy masks historical OP and reply names.

Time labels retain the source's singular wording and omitted one-minute or
one-hour remainder. The source stores text-only OP dates as formatted `now`
strings but subtracts them as numbers in the hover code. Its resulting `NaN`
falls through to `one day ago`; that observable spelling is retained. Last-reply
dates are numeric on both board types and use the ordinary relative-age path.

The preview uses the source's 500px maximum content width, 10pt Arial, padding,
colors and rounded background. It sits 5px beside the hovered element, choosing
the left side when less than 30% of viewport width remains on the right. Near
the bottom it moves upward with the source's 20px margin, clamps a negative top
to 3px and adds document scroll offsets. No unrelated page CSS was restyled.

## Data and security boundaries

The existing repeatable-read catalog snapshot fetches at most one public header
per selected thread in one additional query. It selects only thread ID, reply
ID, name and creation time using the latest visible IDs from the same snapshot.
It does not fetch reply bodies, add per-thread queries or extend the JSON API's
existing preview loads. Catalog HTML still loads only each OP's comment body.

Askama escapes user fields in an inert template inside each card or text row's
control cell. The browser clones that finite template and fills only text and
numeric positions. It never reparses user strings as HTML. Existing typed
formatting, CSP, URL checks, deletion visibility and storage bounds remain.
This does not add database privileges or any media/staff authority.

## Verification and limits

`catalog_controls.rs` uses the real public database role to check latest-reply
selection, deletion fallback, empty replies, escaped names and timestamps,
current forced-anonymous policy and OP-only body loads. The persisted browser
case posts image- and text-board threads, hovers actual elements, checks literal
hostile-looking names/comments, deletes replies and verifies the rendered
fallback without script execution or CSP violations.

Eleven focused theme/browser cases cover the exact delay, cancellation, source
duration boundaries, page numbering after sort/pin/filter, real scroll offsets,
text-date behavior and six theme colors. Seven new Windows Chromium captures
were inspected for readable headers, teaser wrapping, page labels and last-reply
lines. Existing captures were not refreshed. The fixture set includes a separate
two-threads-per-page catalog so page-number assertions exercise later pages.

The initial checks exposed test assumptions about Askama's numeric escaping,
the legacy fixture's stripped spoiler tag, a missing period, clock-boundary
rounding and image-catalog sticky priority. Those assertions were corrected
against the real formatter/source behavior. An old local database connection
also timed out; the persisted tests passed using the active disposable database.
Full current-head CI remains required before merge.

Local Windows checks passed `cargo test -p board-public --all-features --locked`,
strict Clippy for public/store all targets and features, formatting, release
bundle consistency and JavaScript syntax. The three persisted preview/teaser/
text-catalog browser cases passed. Without updating baselines, the complete theme
suite passed 148 cases, basic visuals 3, media visuals 39, archive visuals 6 and
empty/error states 10. These results do not qualify Linux-only execution.

This advances V-005/V-009, not complete catalog parity. Tripcodes, capcodes,
country flags and custom anonymous labels depend on identity/board features not
implemented here. The surrounding page, complete native theme/highlight options,
source link/filename spelling and ambient encoding remain unfinished. No
migration or dependency change is needed. Deploying the public binary supplies
the new templates and fixed assets; no production deployment is performed.
