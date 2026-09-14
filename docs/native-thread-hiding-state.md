# Native thread-hiding state

The state module is integrated into the fixed browser bundle. Board indexes have
desktop minus/plus controls, an OP menu action and a mobile `Show Hidden Thread`
restoration button. Thread pages do not hide their own thread. Settings exposes
the native default-enabled option, hide-stub preference and `Clear History`.
The targeted tests below cover filter precedence, delayed cleanup and all six
themes, but are not a claim of complete reference parity. Reviewed public-page
comparisons and the remaining native-extension feature work are still open.

## Public evidence

The permitted reference is the public extension script
`https://s.4cdn.org/js/extension.min.1191.js`, collected on
2026-09-13 at 12:55:46.721 UTC. Its SHA-256 is
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
The script was inspected as text, not executed.

`ThreadHiding` stores a board-specific ID-to-timestamp map under
`4chan-hide-t-<board>`. Hiding or restoring a saved hide during page parsing renews
its timestamp. Unhiding removes the entry, and an empty map removes the key.
Settings defaults thread hiding to enabled and offers `Clear History`.

The twelve-hour threshold belongs to `4chan-purge-t-<board>`, not the age of an
individual hidden thread. Once that timestamp is strictly older than twelve
hours, a successful live-thread listing retains only matching IDs, sets their
values to `1`, saves the map, and advances the purge timestamp. The original
requests `/<board>/threads.json`; a failed request does not clear saved hides.
Ordinary reply hiding's seven-day expiry does not apply here.

## Bounded implementation contract

The state module accepts canonical positive signed-i64 IDs as strings and
nonnegative safe-integer timestamps. It shares the reply-record shape validator,
but never its expiry function. Records are limited to 512 entries and 65,536
UTF-16 code units. Invalid values produce no replacement storage value.

Cleanup consumes a single successful board result from the existing bounded
`NativeCatalogTransport`, using its read-only `/_watch/<board>/catalog.json`
alias. This endpoint substitution reuses the existing credentials-omitting,
same-origin transport and its parser and resource budgets; only thread IDs are
used. Failed, partial, excessive, duplicate-ID or wrong-board results do not
produce a write plan. An empty list is authoritative only after successful
transport and parsing.

A cleanup plan captures both original storage strings. Completion runs
under a board-specific Web Lock, rereads both values inside the lock, and abandons
the write when either has changed. The caller also checks current
settings and page lifetime. Exact string comparisons intentionally reject even
format-only edits. The caller must write the hidden map before the successful
purge timestamp; the two localStorage keys are not an atomic transaction.
No claim is made that a value changed and then restored byte-for-byte can be
detected with these native keys alone.

The Node tests cover native timestamps, exact IDs, bounds, interval edges,
renewal without age expiry, successful pruning, authoritative empty results,
failed and malformed results, and stale snapshots. Eleven persisted-browser tests
cover desktop hiding and restoration, mobile viewport transitions, cross-tab OP
menus, Settings-based recovery with hidden stubs, successful real live-list
cleanup, invalid records, an injected unavailable-lock fallback and cancellation
behind a real Web Lock. Additional cases distinguish Hide filters from Highlight
filters and hold the real browser lock while a failed or stale successful catalog
response finishes. Neither response may erase the current history or advance the
cleanup timestamp. The lockless fallback is a fault-injection test, not evidence
of actual browser storage denial.

`Clear History` confirms the current board's count and removes the hidden record;
as in the pinned function, it does not immediately rebuild the current page.
Saving extension Settings navigates and restores the page. Sticky threads keep
desktop stubs even when ordinary stubs are disabled. The sixteen minus/plus
images are unchanged public PNGs recorded in `public-watcher-assets.json` for
the four theme families and both pixel densities.

Twelve theme tests exercise desktop and mobile controls in all six themes at
both pixel densities and attach 24 implemented-state captures. Six existing
post-menu theme tests cover menu placement and watch actions alongside the new
Hide action. These project captures remain distinct from public-reference
captures. The desktop project baseline gains the default-enabled minus control;
the mobile board and catalog baselines do not change.

The mobile restoration button uses the pinned `.mobile-tu-show` 150-pixel
content width and centered margins, plus the public mobile stylesheet's rounded
button, padding, bold inherited font and worksafe-specific gradient/colors.
Explicit content-box sizing preserves the reference span's dimensions while
retaining a semantic keyboard-operable button. Theme tests assert a 172-pixel
outer width, the worksafe colors and a single-line label, rather than accepting
the form-control sizing that originally wrapped the label in four themes.
