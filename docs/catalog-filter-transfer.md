# Catalog filter transfer

Settings export and restore now include the public `catalogFilters` field.
It maps to `catalog-filters`, preserving the reviewed numeric-key object and
its admitted raw text. Catalog rules keep their own grammar; native post-filter
arrays are neither converted nor accepted as catalog rules. Empty `{}` clears
catalog rules. `catalog-theme`, watched threads, deletion passwords, receipts
and upload capabilities remain outside the transfer envelope.

The existing review shows catalog rule count and exact text before any write.
The adapter checks active pattern syntax in a fresh catalog worker, including
rules scoped to other boards, using an empty card batch. It then acquires the
same Web Lock as the catalog editor and compares every included opening value.
All included data is written before native enabling preferences. Failed writes
recover only values still owned by this restore; a replacement from another
writer is preserved and incomplete recovery is reported.

On a catalog, successful restore refreshes the matching snapshot and applies
the display in one content insertion. Existing pins and unrelated theme text
remain intact. Malformed preexisting display preferences do not prevent valid
transferred rules from taking effect. The existing finite display remains in
use with an explicit message. A native-only restore preserves unrelated
tab-only catalog rules; an explicit catalog restore can refresh that consumer's
read cache without granting it persistence authority. An already-open rule
editor must reopen before saving its stale draft.

See [settings transfer](native-settings-transfer.md) for bounds, supported
keys, deliberate review/validation differences and the lifetime contract.
The fixed catalog-core module and worker are admitted on interactive public
pages for transfer validation. Worker response policy still denies network
access, imports and nested workers. There are no new origins, dependencies,
migrations, credentials or staff/media permissions.

## Independent reference

[The collector](../scripts/verify-public-settings-transfer-reference.mjs)
checks the existing 182,061-byte v1191 pin with SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
It loads the whole unchanged public client on an owned empty shell after
DOMContentLoaded. The actual `Main.init` completes; the board's `Main.run`
listener does not execute. Calls to the released export and restore entry
points record five export states and four restore states, including absent and
empty optional fields, all five fields, empty catalog rules and numeric rule
keys. Nonfixture traffic is denied and every case checks page errors.

[The record](public-settings-transfer-reference.json) is protocol evidence.
It does not qualify original-page export geometry, focus or board startup.
The public restore saves a merged configuration automatically and accepts raw
optional values. The rewrite requires review, validates each finite format,
stores the reviewed settings object and writes enabling settings last. These
differences are recorded rather than presented as identical persistence.

## Qualification

The final Windows run passes all 38 PostgreSQL-backed browser cases across
catalog transfer, existing native transfer, catalog preference locking and
both worker/CSP suites. The new catalog case creates two owned threads and
deletes them through the real public handlers. All 26 transfer and preference
lock core cases pass on Windows and Linux. Linux also passes all 35 catalog
filter core cases, 93 public library tests, nine fixed-asset cases and public
all-target/all-feature Clippy. Windows passes its 89 public library tests,
nine asset cases and the same Clippy command. Formatting and diff checks pass.

Before the final malformed-display and event-key changes, all 272 selected
Windows production-template cases passed across catalog layouts, controls,
filters, Settings, shortcuts, spoilers, previews, text catalogs and watcher
presentation. The final production run separately checks the changed restore
behavior and the unchanged negative CSP assertions. The final Linux database
browser run passes all 38 cases in 2.9 minutes; Windows passes them in 32.9 seconds.

Failures were recorded and corrected:

- The first Windows production run passed 34 of 35. Its new fixture assumed
  two preexisting demo threads, but the local board contained one. It now creates
  and removes its own threads. All nine transfer cases then passed.
- The next run passed 35 of 36. Its new render observer counted both moving a
  card into a fragment and inserting the completed catalog. It now counts
  completed content insertions and still requires exactly one per restore.
  All 36 passed before adding the malformed-display and tab-only cases; the
  final expanded run passes all 38. Failure traces remain in ignored local
  artifacts.
- The first Linux core run passed the library, assets, Clippy, transfer and
  filter checks, then failed all eight preference-lock core cases. That older
  owned fixture served only two modules and omitted the new fixed imports.
  It now serves the complete seven-module dependency set and rejects unexpected
  fixture requests immediately. All eight pass on both platforms, without
  changing their lock, storage, cancellation or lifetime assertions.

No retries, browser/test timeouts, pixel tolerance or screenshot baselines were
changed. These browser checks make no new media-containment claim. The combined
catalog batch still requires hosted checks before merge; complete page chrome
and server teaser normalization remain unfinished.

Successful commands include:

```text
node scripts/verify-public-settings-transfer-reference.mjs .local/references
node --test tests/browser/native-settings-transfer.test.mjs tests/browser/catalog-preference-locks.test.mjs
npm run test:catalog-filters-core
cargo test -p board-public --lib --locked
cargo test -p board-public --test ui_assets --locked
cargo clippy -p board-public --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
npx playwright test tests/browser/catalog-filter-transfer.spec.js tests/browser/native-settings-transfer.spec.js tests/browser/catalog-preferences.spec.js tests/browser/catalog-preference-locks.spec.js tests/browser/native-filter-worker.spec.js tests/browser/catalog-filter-worker.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/catalog tests/themes/text-catalog.spec.js tests/themes/thread-watcher.spec.js tests/themes/watcher-icons.spec.js
```

The transfer cases are included in `npm run test:settings-transfer`, which the
Linux verification workflow runs. Windows continues to run the core and
production-template suites. The public replay also passed after writing its
new record; it needs the existing ignored pinned-reference directory.
