# Native settings transfer

The pinned public extension v1191 exports settings in the URL fragment `#cfg=`. The fragment contains `encodeURIComponent(JSON.stringify(payload))`, where the payload has this public shape:

```text
{
  settings: raw settings JSON,
  filters?: raw filter JSON,
  css?: custom CSS text,
  catalogFilters?: raw catalog filter JSON,
  catalogSettings?: raw catalog settings JSON
}
```

The public Export Settings panel shows the generated URL in a read-only field and provides a bookmarkable restore link. The rewrite keeps that URL shape and the export control in board/thread Settings. The catalog uses its separate [public catalog editor](catalog-settings.md). The export builder uses the canonical current board URL (`/<board>/`) as the destination, so any supported caller does not copy a query string, fragment, thread identifier, or upload URL into the bookmark.

Only preference storage is considered for transfer. The module reads `4chan-settings`, `4chan-filters`, `4chan-css`, `catalog-filters`, and `catalog-settings`. It does not enumerate storage, cookies, forms, or page fields, and it never reads Thread Watcher state, tracked-post state, post receipts, deletion passwords, upload identifiers, upload capabilities, or other workflow data. `catalog-theme` is outside the pinned payload and remains excluded.

## Compatibility boundary

The rewrite has a finite `catalog-settings` format: `orderby` is one of `alt`, `absdate`, `date`, or `r`, and `large` and `extended` are booleans. That format is exported and restored.

The `catalogFilters` field now exports and restores the separate
[catalog rule format](catalog-filters.md): an object with numeric rule keys,
bounded patterns and board scopes, boolean or numeric flags, and validated
colors. Empty `{}` clears catalog rules. Raw admitted text is preserved;
catalog rules are not converted into native post filters. Arrays, malformed
rules, reserved nested properties and unsafe colors reject the complete payload.

[The public replay](../scripts/verify-public-settings-transfer-reference.mjs)
loads the whole unchanged v1191 client on an owned empty shell after
DOMContentLoaded. `Main.init` completes; the board's `Main.run` listener does
not execute. Direct `Config.toURL` and `Config.loadFromURL` calls independently
record five exports and four restores, including all five fields, missing and
empty optional values, empty catalog rules and sparse numeric keys. The record
checks the pinned client hash, read/write keys and denied nonfixture traffic.
It does not qualify original-page export layout, focus or board initialization.

The public restore merges settings into its current full configuration and
saves settings before optional fields. The rewrite stores the reviewed finite
settings object and writes enabling preferences last. It also requires review
and validates every optional field before saving. An empty optional export is
omitted; an explicitly transferred empty catalog-rule string is invalid rather
than being treated as permission to remove stored rules. These boundaries are
visible in the replay and the rewrite's validation tests.

The v1191 defaults include `forceHTTPS` and `unmuteWebm`. They are preserved as boolean compatibility values, but this version does not activate those features. `IDColor` is active for [poster IDs](poster-ids.md). The review marks them as inactive compatibility values. In particular, a transferred `forceHTTPS` value does not relax the rewrite's HTTPS policy.

Quick Reply accepts both the rewrite's finite `{left, top}` coordinates and the public client's bounded CSS-coordinate form, such as `right: 20px; top: 10%;`. The latter passes through the same coordinate parser as movable navigation. Only numeric pixel or percentage offsets are used; arbitrary CSS is rejected. Opening the panel converts offsets for the current viewport and keeps the panel within its edges.

Filter colors pass the browser validator used by the native filter editor. After confirmation, active patterns are syntax-checked in their respective disposable workers with an empty post/card batch, before any preference write. Catalog validation checks active rules scoped to other boards as well, using an all-board validation packet while preserving the reviewed scope in storage. Invalid patterns, a failed worker or cancellation leave stored preferences unchanged. This check does not execute a pattern against post text. Export is unavailable while a Settings save is pending, so its link cannot capture the previous values during a queued save.

All other settings use the current finite client formats. Boolean settings must be booleans. `customMenuList` uses the existing custom-board validator. Thread Watcher and native navigation positions use the existing bounded position parser. Quick Reply accepts the coordinate forms described above. Unknown setting names and reserved object keys such as `__proto__`, `prototype`, and `constructor` are rejected.

Native filters must pass both current native filter readers, including their 64-rule and field limits. Catalog filters pass their own 64-rule reader and the same color validator. Custom CSS must pass the native Custom CSS parser and its safe selector/property/value grammar. Catalog settings must match the exact three-field format above.

## Bounds and parsing order

Transfer parsing is intentionally staged so a link cannot force unbounded decoding or nested parsing:

- The complete `#cfg=` fragment is rejected before slicing or decoding if its encoded payload exceeds 1,048,576 characters.
- The decoded outer JSON is limited to 393,216 characters and 524,288 UTF-8 bytes before the outer `JSON.parse` call.
- The settings string is limited to 4,096 characters before parsing.
- The filter string is limited to 131,072 characters before parsing.
- The catalog filter string has its own 131,072-character ceiling before parsing; patterns and board scopes are each limited to 1,024 characters per rule.
- Custom CSS is limited to 16,384 UTF-8 bytes by the existing CSS validator.
- Catalog settings are limited to 1,024 characters before parsing.
- Parsed object traversal is iterative and capped at 4,096 object/array nodes.

The complete incoming data must pass these format and color checks before a review opens. A bad optional field rejects the entire payload. Pattern syntax is checked in the disposable worker after confirmation, before any write. Existing raw values serve only as change-detection snapshots: each has a separate 4,194,304-character ceiling and need not be valid, so restore can replace malformed or oversized preferences with valid incoming values.

## Export and review

The export panel is built with DOM methods and `textContent`. Stored filter patterns or other strings never become HTML. The URL field is read-only, and the Restore Settings bookmark points to the same canonical board URL shown in the field.

Opening a valid restore URL removes the `#cfg=` fragment from the visible address and opens a review dialog. It does not write storage. The review lists every settings value that would be restored and provides expandable text views of the exact filter and Custom CSS strings plus catalog settings when present. The filter list is capped by the filter format, the CSS text by its 16 KiB limit, and the number of settings rows by the finite setting whitelist.

This confirmation step is a deliberate security difference from v1191. A `#cfg=` link can arrive from an untrusted source, so visiting it is not treated as permission to replace browser preferences. The user must choose **Restore Settings** after reviewing the values.

## Restore transaction contract

`mountNativeSettingsTransfer({ root, readItem, restore })` is storage-agnostic. The module passes the reviewed values and a snapshot of the current raw values to the shared adapter:

```text
restore(values, expected, signal)
```

`values` contains only `4chan-settings`, `4chan-filters`, `4chan-css`, `catalog-filters`, and/or `catalog-settings`, with `4chan-settings` always present. `expected` contains the raw current value, or `null`, for each included key. `checkTransferValues(values)` is exported so the shared adapter can revalidate the complete set immediately before queuing the write.

The shared adapter owns locking and storage mutation. It requires persistent browser storage and the shared Web Lock, compares every included key with `expected`, and returns `conflict` if anything changed after the review opened. A successful callback writes the included keys while holding that lock. Web Storage has no multi-key transaction primitive, so this is not described as atomic. If a write fails, recovery restores only values that still equal this restore's writes, preserving replacements from nonparticipating writers and reporting incomplete recovery.

The UI handles these adapter results explicitly:

- `ok`: keep the review open, show **Settings restored**, emit `4chanPreferencesRestored` and `4chanSettingsSaved`, and disable the Restore button.
- `conflict`: keep the newer stored values and require a fresh review.
- `invalid`: report that the settings restore was rejected.
- `unavailable`: report that persistent storage or cross-tab locking is unavailable and write nothing.
- `storage-error` with `partial: false`: report the failed write and successful rollback.
- `storage-error` with `partial: true`: warn that rollback could not completely recover the old stored values.

The `4chanPreferencesRestored` event is the cross-feature notification used by the native settings and catalog preference owners. Its detail contains the persistence result and the included preference keys. Catalog rules refresh only when their key was restored, preserving unrelated tab-only rules during a native-only restore. `4chanSettingsSaved` remains the normal settings-refresh signal. The transfer module itself does not interpret these events as authority to mutate other storage.

On an open catalog, the owner awaits a fresh filter snapshot before applying
restored display preferences, producing one catalog render. Pins, manual hides,
search state and unrelated theme text retain their existing owners. An already
open catalog filter editor must reopen before saving its stale draft. A
tab-only consumer can refresh its read cache after a successful persistent
restore without gaining permission to persist future edits.

The fixed catalog-core module and worker are now admitted on interactive public
pages so board/thread restore can validate catalog rules. The script and worker
lists still contain exact release-owned paths, with no general `self` worker
permission. Worker response CSP continues to deny network access, imports and
nested workers. Catalog editor/theme paths remain catalog-only; API and staff
listeners gain no new path or origin.

## Lifetime

Closing a restore review aborts an outstanding restore. A persisted `pagehide` also aborts it and removes the dialog while retaining the reviewed snapshot; a persisted `pageshow` reopens the same review. If another tab changed a reviewed value while the page was in the back-forward cache, the original `expected` snapshot makes the next Restore attempt conflict instead of overwriting the newer value.

Removing the mounted root destroys the controller, closes its dialog, and aborts pending work. A final `pagehide` does the same. Duplicate mounts retire the earlier controller. Storage changes refresh an open export URL, while a restore review deliberately keeps its original snapshot so cross-tab changes are detected by compare-and-swap.

## Verification

`tests/browser/native-settings-transfer.test.mjs` covers the public payload shape, full v1191 default settings, compatibility-only values, pre-decode and pre-parse bounds, reserved keys, filter/CSS/catalog validators, export key isolation, canonical URLs, inert DOM, explicit review, stale-value conflicts, delayed cancellation, root detachment, BFCache, and adapter failure messages.

`tests/browser/native-settings-transfer.spec.js` exercises the real public application and shared storage adapter. It checks the Settings Export button, canonical export URL, exclusion of unrelated state, explicit review before writes, disabled-native-settings restore, cross-tab conflicts, malformed catalog filters, dangerous CSS without network activity, quota failure with rollback, and unavailable lock/storage paths.

`tests/browser/catalog-filter-transfer.spec.js` covers actual catalog refresh,
clearing, pins, stale editor drafts, other-board syntax checks, cancellation,
detachment, BFCache, shared-lock conflicts and five-key rollback. Its catalog
case creates and removes two owned threads rather than assuming a fixed number
of posts already exists. Commands and platform results are recorded in
[catalog transfer qualification](catalog-filter-transfer.md).
