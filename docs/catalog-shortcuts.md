# Catalog keyboard shortcuts

Catalog shortcuts now run on keyup, as in the pinned public client. Input and
textarea targets are excluded. S opens Search or clears its open field and
labels while retaining the applied search until the next field keyup. Button,
link and select targets are eligible. Ctrl, Alt, Shift and Meta do not suppress
S. The event is not marked prevented.

X cycles Bump order → Reply count → Last reply → Creation date → Bump order.
It uses the existing bounded catalog sort, preference lock and local URL update.
R navigates to the current URL to refresh the document; Shift suppresses R.
The bounded `catalog-theme.nobinds` preference suppresses all three shortcuts.
Malformed or unavailable preference storage leaves keyboard controls usable.

The earlier Search reference used complete S presses from the body and modified
keydown events. Those cases did not establish keyup modifier handling or other
targets. Its documentation previously overstated that evidence as a body-only,
unmodified shortcut. This continuation corrects both the handler and that claim.

## Reference and verification

[The independent replay](../scripts/verify-public-catalog-settings-reference.mjs)
runs the whole unchanged public catalog client v1025 on three owned cards. It
checks the released script and six catalog stylesheets by byte count and hash,
denies every nonfixture request and checks page errors after each case.
[Its record](public-catalog-settings-reference.json) contains 50 shortcut states
and 16 refresh cases at widths 1280/390. It records event phase, target,
modifiers, cancellation, Search state, sort order and exact card IDs. Refresh
cases cross the real browser navigation boundary to the same owned document.

The same record now contains 24 Settings editor states, 16 native-default states
and 12 style cases. [Catalog Settings](catalog-settings.md) implements and
qualifies those properties separately from the shortcut tests.
No production post, filename, media or private source fixture is retained.

The production-template shortcut suite passes all 38 tests on Windows. Combined
with the existing Search, filter lifecycle and text-catalog checks, all 95 cases
pass in 47.1 seconds. The owned Settings fixture presents the same exact IDs in
the default rank order used by the reference. Existing Filter and Search
fixtures retain their original order and data. No screenshot baseline, tolerance,
retry or timeout changes.

The owned Linux run passes all 78 shortcut and existing Search cases, with zero
page errors in the shortcut cases. Public all-target/all-feature Clippy passes
on both Windows and Linux; formatting and whitespace checks pass. Hosted checks
remain required before merging the combined batch.

No new asset path, CSP permission, dependency, migration, service authority or
credential policy changes. The three shortcuts require complete catalog metadata
and the already-admitted Search controls. Complete original-page comparisons,
complete navigation chrome and server normalization remain unfinished.
