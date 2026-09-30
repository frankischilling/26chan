# Catalog preference storage locking

Catalog sort, image-size and teaser controls still update the current catalog
immediately. Their one persistent record, `catalog-settings`, now uses the same
`paperboard-thread-watcher` Web Lock as settings transfer and the other shared
browser preference mutations.

Each save or Reset reads the exact raw `catalog-settings` value before it queues.
After the shared lock is acquired, the callback reads the key again and writes
only when the raw value still matches. A restore that completes while an older
catalog choice is waiting therefore wins; the old choice reports a conflict and
cannot overwrite restored preferences. Unrelated catalog storage, including
search session state, spoiler choice, hidden threads and pinned threads, keeps its
existing persistence path.

Starting another local catalog save or Reset aborts the earlier pending action.
The lock helper keeps acquisition bounded to its existing five-second deadline.
A persisted `pagehide` aborts queued work and suspends the helper; a persisted
`pageshow` resumes it. A terminal page exit stays suspended. The synchronous
locked callback also verifies that the original form and controls remain live,
so detached or replaced catalog controls cannot commit a queued write.

Cross-tab `catalog-settings` storage events cancel an outstanding local write and
leave the displayed catalog unchanged. A small `role="status"` element beside
the controls is hidden while empty and explains busy, conflict, storage-denied
or lock-denied persistence. The display itself remains usable when persistence
is unavailable.

Settings transfer dispatches `4chanPreferencesRestored` after its transaction.
The catalog handler cancels any pending local write, reads and validates the
stored `catalog-settings` record, and applies those three display fields to the
existing snapshot. It updates the URL with `history.replaceState`; it neither
writes storage nor navigates. The active search text and in-memory hidden/pinned
state are retained. Invalid or unavailable restored catalog storage is left
unapplied and is reported through the status element.

The fixed catalog script is now a module and imports `NativeWatchLock` from the
already permitted `/static/native-filter.v1.js` bundle. No new script origin,
network endpoint, database state or server-side preference is introduced.

`tests/browser/catalog-preference-locks.test.mjs` is the standalone Chromium
race suite. It covers a held cross-tab lock, latest-action cancellation, raw CAS
after an unseen storage change, cross-tab storage cancellation, restore-event
application without writes, retained search/hidden/pinned state, BFCache
suspension and resumption, detached controls, the bounded busy path, and denied
storage or Web Locks.

`tests/browser/catalog-preference-locks.spec.js` exercises the shared lock and
restore event against the real catalog page as part of the public browser suite.
