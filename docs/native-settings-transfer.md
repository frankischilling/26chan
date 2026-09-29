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

The public Export Settings panel shows the generated URL in a read-only field and provides a bookmarkable restore link. The rewrite keeps that URL shape and the export control. It uses the canonical current board URL (`/<board>/`) as the destination, so an export opened from a thread, catalog page, or upload flow does not copy the current query string, fragment, thread identifier, or upload URL into the bookmark.

Only preference storage is considered for transfer. The module reads `4chan-settings`, `4chan-filters`, `4chan-css`, and `catalog-settings`. It does not enumerate storage, cookies, forms, or page fields, and it never reads Thread Watcher state, tracked-post state, post receipts, deletion passwords, upload identifiers, upload capabilities, or other workflow data.

## Compatibility boundary

The rewrite has a finite `catalog-settings` format: `orderby` is one of `alt`, `absdate`, `date`, or `r`, and `large` and `extended` are booleans. That format is exported and restored.

The public payload also has a `catalogFilters` field. The rewrite has no current `catalog-filters` consumer or validator; catalog filtering uses the validated `4chan-filters` format instead. Generated exports therefore omit `catalogFilters`, and incoming payloads that contain it are rejected with an explicit message. No conversion between the two formats is guessed.

The v1191 defaults include `IDColor`, `forceHTTPS`, and `unmuteWebm`. They are preserved as boolean compatibility values, but this version does not activate those features. The review marks them as inactive compatibility values. In particular, a transferred `forceHTTPS` value does not relax the rewrite's HTTPS policy.

Quick Reply accepts both the rewrite's finite `{left, top}` coordinates and the public client's bounded CSS-coordinate form, such as `right: 20px; top: 10%;`. The latter passes through the same coordinate parser as movable navigation. Only numeric pixel or percentage offsets are used; arbitrary CSS is rejected. Opening the panel converts offsets for the current viewport and keeps the panel within its edges.

Filter colors pass the browser validator used by the native filter editor. After confirmation, active patterns are syntax-checked in the existing disposable worker with an empty post batch, before any preference write. Invalid patterns, a failed worker or cancellation leave stored preferences unchanged. This check does not execute a pattern against post text. Export is unavailable while a Settings save is pending, so its link cannot capture the previous values during a queued save.

All other settings use the current finite client formats. Boolean settings must be booleans. `customMenuList` uses the existing custom-board validator. Thread Watcher and native navigation positions use the existing bounded position parser. Quick Reply accepts the coordinate forms described above. Unknown setting names and reserved object keys such as `__proto__`, `prototype`, and `constructor` are rejected.

Filters must pass both current filter readers, including their 64-rule and field limits. Custom CSS must pass the native Custom CSS parser and its safe selector/property/value grammar. Catalog settings must match the exact three-field format above.

## Bounds and parsing order

Transfer parsing is intentionally staged so a link cannot force unbounded decoding or nested parsing:

- The complete `#cfg=` fragment is rejected before slicing or decoding if its encoded payload exceeds 1,048,576 characters.
- The decoded outer JSON is limited to 393,216 characters and 524,288 UTF-8 bytes before the outer `JSON.parse` call.
- The settings string is limited to 4,096 characters before parsing.
- The filter string is limited to 131,072 characters before parsing.
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

`values` contains only `4chan-settings`, `4chan-filters`, `4chan-css`, and/or `catalog-settings`, with `4chan-settings` always present. `expected` contains the raw current value, or `null`, for each included key. `checkTransferValues(values)` is exported so the shared adapter can revalidate the complete set immediately before queuing the write.

The shared adapter owns locking and storage mutation. It requires persistent browser storage and the shared Web Lock, compares every included key with `expected`, and returns `conflict` if anything changed after the review opened. A successful callback writes the included keys while holding that lock. Web Storage has no multi-key transaction primitive, so this is not described as atomic. If a write fails, the adapter attempts to restore values it already changed and reports whether that rollback was complete.

The UI handles these adapter results explicitly:

- `ok`: keep the review open, show **Settings restored**, emit `4chanPreferencesRestored` and `4chanSettingsSaved`, and disable the Restore button.
- `conflict`: keep the newer stored values and require a fresh review.
- `invalid`: report that the settings restore was rejected.
- `unavailable`: report that persistent storage or cross-tab locking is unavailable and write nothing.
- `storage-error` with `partial: false`: report the failed write and successful rollback.
- `storage-error` with `partial: true`: warn that rollback could not completely recover the old stored values.

The `4chanPreferencesRestored` event is the cross-feature notification used by the native settings and catalog preference owners. `4chanSettingsSaved` remains the normal settings-refresh signal. The transfer module itself does not interpret these events as authority to mutate other storage.

## Lifetime

Closing a restore review aborts an outstanding restore. A persisted `pagehide` also aborts it and removes the dialog while retaining the reviewed snapshot; a persisted `pageshow` reopens the same review. If another tab changed a reviewed value while the page was in the back-forward cache, the original `expected` snapshot makes the next Restore attempt conflict instead of overwriting the newer value.

Removing the mounted root destroys the controller, closes its dialog, and aborts pending work. A final `pagehide` does the same. Duplicate mounts retire the earlier controller. Storage changes refresh an open export URL, while a restore review deliberately keeps its original snapshot so cross-tab changes are detected by compare-and-swap.

## Verification

`tests/browser/native-settings-transfer.test.mjs` covers the public payload shape, full v1191 default settings, compatibility-only values, pre-decode and pre-parse bounds, reserved keys, filter/CSS/catalog validators, export key isolation, canonical URLs, inert DOM, explicit review, stale-value conflicts, delayed cancellation, root detachment, BFCache, and adapter failure messages.

`tests/browser/native-settings-transfer.spec.js` exercises the real public application and shared storage adapter. It checks the Settings Export button, canonical export URL, exclusion of unrelated state, explicit review before writes, disabled-native-settings restore, cross-tab conflicts, unsupported catalog filters, dangerous CSS without network activity, quota failure with rollback, and unavailable lock/storage paths.
