# Watcher storage deadlines

Watch changes, settings, position saves, posting receipts, read acknowledgements
and refresh claims share the `paperboard-thread-watcher` Web Lock. Each request
has a five-second acquisition deadline, and each page admits at most 16 pending
actions. Expiry reports a busy-storage message and releases the pending slot.
A late lock callback cannot apply an expired action. Users can retry after the
other tab releases the lock.

Caller cancellation and `pagehide` revoke waiting actions. A persisted
`pageshow` enables fresh actions without reviving cancelled callbacks. The
protected callbacks perform synchronous, bounded storage mutations; this helper
does not provide deadlines for asynchronous work inside a callback. Network and
filter work retain their separate response, concurrency and elapsed-time bounds.

Lock availability is independent of watcher storage availability. Failure to
read `4chan-watch` must not allow independently healthy posting-receipt storage
to write without acquiring the lock. Browsers without Web Locks retain the
existing explicit same-tab storage fallback. If browser policy exposes Web Locks
but rejects acquisition with `SecurityError`, all shared participants switch
to memory before an unlocked callback runs, including posting tracking and
refresh timestamps. Other acquisition failures do not run the callback.
An error from inside an acquired lock does not trigger fallback or replay the
mutation.
This change does not make writes across separate storage keys atomic.

A settings save that times out retains the dialog draft and allows another
attempt. An expired receipt-consumption attempt retains the receipt cookie for
a later navigation. An expired refresh claim does not start network work or
report the separate one-minute refresh cooldown. Page-exit cancellation also
prevents settings and initial receipt continuations from restarting navigation
or refresh work.

These deadlines and pending-action limits are resource and concurrency
safeguards in the rewrite. The supplied old `js/extension.js` watcher rules and
the separately pinned public extension establish the watch behavior, not these
additional limits. See the [source audit](compatibility.md#native-extension)
and [watcher contract](thread-watcher.md).

`tests/browser/native-watch-lock.test.mjs` exercises hung and failed providers,
late callbacks, cancellation, resumption, queue saturation and recovery.
`tests/browser/watcher-locks.spec.js` uses actual cross-tab Web Locks and
persisted synthetic posts to exercise watch/settings expiry, cancelled unwatch
and suppression, retained posting receipts, partial storage failure, policy-denied
locking with healthy storage, callback errors and stale refresh prevention.
Its lifecycle case dispatches page-transition events; it
does not qualify whole-application browser back/forward-cache behavior. Both
suites run through `npm run test:behavior`.

Local Windows/Chromium/PostgreSQL validation passed 125 unit/state checks and
157 persisted browser cases through `npm run test:behavior`, including all
eight lock cases. The bundle reproduces at 221,981 bytes with
`npm run check:native-filter`. `cargo build -p board-public --examples --locked`
also passed. The first full browser run caught the policy-denied Web Locks
regression; the fix retains the existing cookie-policy assertions and adds
healthy-storage and callback-error controls. Hosted checks must qualify the
committed head separately.
