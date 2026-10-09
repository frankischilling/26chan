# Local blotter

Board index and open-thread pages show the three newest published messages, ordered by ID descending. The preview follows the ordinary posting fields inside the form, is omitted when empty, and is disabled on `/j/`. Catalogs and closed or archived threads omit it. The preview is desktop-only, matching the pinned source; `/blotter` remains readable on mobile.

Hide stores the newest displayed Unix timestamp in `4chan-blotter`. Equal or older messages stay hidden on reload. A newer publication shows the preview again. Show Blotter clears the saved value and restores Show All. Denied storage leaves the controls usable for the current page. Invalid, oversized or nondecimal saved values are ignored. Without JavaScript, messages and Show All remain visible on desktop.

`/blotter` is a local, read-only dated table with `msg-<id>` anchors. Each page has at most 25 messages. Next uses `/blotter?offset=<last displayed ID>` and selects smaller IDs; it does not use SQL OFFSET. Dates use UTC. The shared page shell supplies local navigation, styles and the public response security policy.

## Publishing

The `board-blotter` executable belongs to `board-store`. Prepare a UTF-8 JSON file:

```json
{"version":1,"published_at":1791504000,"content":"An operator announcement.\nA second line."}
```

Run `board-blotter validate message.json` to check the file without database access. `board-blotter publish message.json` requires `MIGRATION_DATABASE_URL` for `board_migrator`. The public and staff database roles cannot publish messages. Publish appends a message; `board-blotter retract <id>` removes it from public reads while retaining its record. Publish a new message for a correction, then retract the old one if needed.

The input file is limited to 32 KiB and content to 8192 UTF-8 bytes. Content permits line breaks and tabs, but rejects other control characters. Publication timestamps must be positive Unix seconds through 9999-12-31, and strictly newer than every retained message, including retracted entries. This preserves new-message visibility after dismissal. The store retains at most 10,000 records.

Messages are plain text. Askama escapes HTML, and CSS preserves line breaks. Source HTML formatting and links are displayed literally. This is an intentional security difference from the reference, which inserts trusted HTML. The implementation imports no reference announcements and has no external feed dependency.

## Reference boundary

The pinned source's `imgboard.php:411-451` supplies SHOW_BLOTTER, newest-three selection, empty omission and the timestamp control. `views/imgboard.php:209-225` supplies placement in the posting branch. `js/core.js:990-1044` supplies dismissal behavior. `www.4chan/views/blotter.tpl.php` supplies the dated table, row anchors and cursor-shaped Next link.

The reference controller, Atom generator and included front-page footer are unavailable. Their semantics are unverified. This implementation has no Atom endpoint or copied footer and uses a local page size and bounds. Show All opens the local page in a new tab with `rel="noopener"`. The toggle retains the source anchor and bracketed controls, adds `aria-expanded`, and remains visible without JavaScript, including its original `href="#"` behavior. Restoring a hidden preview writes its current timestamp, matching source initialization even when an older page replaces a newer saved value.

## Checks

`npm run test:blotter-core` verifies the generated asset and the controller's hide, reload, show, newer-message, malformed-storage and unavailable-storage cases. It also checks template escaping and the existing single MainInit dispatch. `npm run test:blotter` runs isolated browser fixtures with the shipped JavaScript and CSS, including desktop/mobile behavior and operation without JavaScript. These fixtures do not qualify database or server rendering.

`npm run test:blotter-persisted` uses the real public server and prebuilt `board-blotter` executable. It requires an empty, isolated development blotter and loopback `board_migrator` credentials. It publishes messages through the operator command, checks real escaped pages and 25-entry cursor pagination, and exercises dismissal with a later publication. Two randomly named boards and marker-owned messages are cleaned up afterward. It refuses to manufacture an empty state by altering unrelated messages. `scripts/verify.sh` runs both browser suites.

The generated module uses the pinned esbuild version and is registered as `/static/native-blotter.v1.js`. Mounting runs before the existing `4chanMainInit` notification; the controller emits no lifecycle events and repeated mounting does not add another click handler.

## Preview geometry

All six supported pinned theme styles use a 468px preview with automatic margins, 11px cells, a centered 50px date column and right-aligned controls. Futaba and Burichan retain browser-default table spacing, cell padding and divider borders. The other themes use their source 1px table spacing, zero cell padding and theme-colored divider. Photon also retains its source divider without a maximum-width constraint. Plain-text line preservation and long-word wrapping accompany the escaped-content security replacement.

The geometry fixture retains hashed source CSS excerpts for Yotsuba, Yotsuba B, Futaba, Burichan, Photon and Tomorrow. Browser checks compare the local preview against those source rules at desktop widths 1280px and 481px, DPR 1 and 2, then check desktop-only omission at 480px and 375px. These browser checks have been discovered and syntax-checked but have not run locally. Standalone-page pixels remain unqualified.

## Local qualification

Focused Node tests and generated-asset checks pass. PG16 validation passed for the migration, runtime readiness, real-role public HTTP and operator behavior. Store tests cover concurrent publication and board policy changes within the same repeatable-read snapshot. The source inventory passes for 82 boards. The populated-0117 upgrade and route-conflict rollback probe passed through a local SCRAM transport adapter.

Scoped offline Clippy for board-public and board-store, including all targets and features with warnings denied, and formatting checks pass. Public library tests passed 129 cases; one Unix-socket transport case was explicitly filtered because the environment prohibits that transport. Full-workspace validation was unavailable because its dependency set was not cached. Browser execution remains for hosted CI.

The first hosted run exposed a missing CSP entry for the new module imported by the watcher. That blocked MainInit and math startup before the blotter scenarios could run. The native-page policy now admits only the fixed same-origin blotter script on board, catalog and upload pages; archive, search and generic responses retain their narrower policy. The policy regression fails without this entry. Local verification passes 128 public library tests (the same Unix-socket case filtered) and all ten UI asset tests, including blotter GET/HEAD bytes, headers, denied writes and absence from the API listener. All 21 math browser cases then passed on hosted head `fedc448`.

That run passed 11 isolated blotter browser cases. Eight geometry cases stopped
at CSS font-family spelling: the source serialized `arial, helvetica` while the
local style serialized `Arial, Helvetica`. Other compared measurements matched.
The comparison now folds ASCII family-name casing, as required by
[CSS Fonts](https://www.w3.org/TR/css-fonts-3/#font-family-casing), while retaining
family names, order, fallback and every geometry assertion. Two regression
tests reject actual family-list changes; all 30 Node cases pass. The corrected
browser comparison and persisted operator scenario still require hosted
qualification. A separate Windows shard also reported socket 10055; this
comparison correction does not address that transport failure.


Hosted qualification passed all 14 checks on head `4d0ef1e`, including the
persisted operator scenario, all blotter geometry cases and Windows visual
checks. Merging the qualified Settings changes from main required retaining
both independent Tomorrow theme variables: the blotter divider and Settings
list border. The combined head requires a fresh CI run before merge.
