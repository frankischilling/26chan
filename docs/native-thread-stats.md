# Native thread statistics

Thread statistics display the status, reply and image counts, reached limits and page number described by public v1191 `ThreadStats`. The public client creates one statistics row on mobile and two on desktop and refreshes the page number every 180 seconds. Its setting is enabled by default. The released script is pinned in [the watcher asset manifest](public-watcher-assets.json); the `ThreadStats` implementation, `Config` defaults and `SettingsMenu.options` provide the reference.

The native client displays the fields in the same order: `Sticky` when applicable, then `Archived` or `Closed`, then replies, images, and the page number for a live thread. A count whose limit has been reached uses an `em` element and the corresponding public tooltip. Archived threads omit the page field and stop the page refresh timer. Desktop thread pages place copies in both desktop thread navigation rows. Mobile layout places one copy after the last mobile thread navigation row.

The optional `Posters` counter appears between images and page number when the server supplies a complete [private poster count](private-poster-counts.md). Historical, mixed-key and archived threads omit it. The client does not infer identity from names, truncated IDs, rendered posts, hidden nodes or cloned DOM.

## Stats response

The browser reads one same-origin endpoint:

```text
GET /_watch/{board}/thread/{thread}/stats
```

The response is an exact version 1 JSON object:

```json
{
  "version": 1,
  "board": "demo",
  "thread": "123",
  "replies": 12,
  "images": 5,
  "sticky": false,
  "closed": false,
  "archived": false,
  "bump_limited": false,
  "image_limited": false,
  "page": 2
}
```

`board` must match the requested lowercase board slug and `thread` must be its canonical positive decimal string. `replies` is an integer from 0 through 1000. `images` is an integer from 0 through `replies`. Optional `unique_ips` is an integer from 1 through `replies + 1` and is allowed only on live threads; unavailable counts are absent, never null. Status and limit fields are booleans. A live thread has a one-based integer `page` from 1 through 1000. An archived thread has `page: null`. A missing live thread returns HTTP 404.

The server computes the counts, thread flags, limit state, and page from one coherent database view. Reply and image counts exclude the opening post, matching the public ThreadStats counters. `bump_limited` uses the board bump policy. `image_limited` uses the public JSON image-limit policy, including its undead-thread behavior. The page rank uses the live board order of sticky threads first, then `bumped_at` descending, then thread ID descending, grouped by the board's `threads_per_page` value.

## Browser bounds and lifecycle

`native-thread-stats.v1.js` accepts at most 4096 response bytes and 4096 stream chunks and gives each request a five second deadline. Empty chunks count toward the work limit. It allows one request at a time, rejects redirects and response URLs other than the exact same-origin endpoint, sends no credentials, requires `application/json`, decodes UTF-8 strictly, and rejects missing, extra, or incorrectly typed fields. Rendering uses DOM text nodes and `textContent`; response data never becomes HTML.

The setting is enabled unless `threadStats` is explicitly `false`, and `disableAll` overrides it. A successful live response arms one 180 second refresh. An accepted native thread update triggers an immediate stats refresh, including updates that add zero posts. Hidden, offline, or suspended pages cancel the active request and timer. Visibility, connectivity, and persisted-page restoration resume with one fresh request. Archived responses and HTTP 404 stop periodic polling. Final page teardown removes listeners, nodes, timers, and active work.

The layout follows the live mobile preference. At viewport widths up to 480 pixels the client uses the mobile placement unless `4chan_never_show_mobile` is exactly `"true"`. Changes to that preference move the existing coherent snapshot without another network request.

## Local adaptation from public v1191

Public v1191 initially counts rendered reply and file nodes, then receives count and limit fields from the thread updater. It also asks the public cross-origin `threads.json` endpoint for page number. The native feature reads the dedicated same-origin stats object from the start, so hidden posts, expansion clones, filtered nodes, and client decorations cannot change the counters. Page number and limit state come from the same bounded server response as the counts.

The server-rendered thread page contains no statistics markup. JavaScript inserts and removes only the enhancement nodes, so disabling JavaScript leaves the original navigation, posts, forms, and thread content unchanged.

## Application integration

The application serves `/static/native-thread-stats.v1.js` and mounts the controller on thread pages with the existing settings reader, mobile media query, and `4chan_never_show_mobile` preference. The Monitoring panel exposes `threadStats` as a default-on option. Shared board CSS supplies the public `.thread-stats` float and margin on desktop, centered layout on mobile, and the `Replies:`, `Images:`, and `Page:` mobile labels.

The module listens to the existing `boardThreadStateChanged` event, which the updater emits after every accepted snapshot, including one with zero new replies. The public `4chanThreadUpdated` event retains its existing behavior and fires only when replies are added.

## Verification

`tests/browser/native-thread-stats.test.mjs` covers canonical context validation, the exact response schema including the optional count, rejection of unknown identity fields, streamed byte limits, redirects, content type, single-flight admission, deadlines, and cancellation. `tests/browser/native-thread-stats-dom.test.mjs` covers desktop and mobile placement, count removal, status and limit emphasis, default-on settings, cross-lifecycle cleanup, archived polling shutdown, and preservation of the last good DOM after invalid fields or partial JSON. `tests/browser/native-thread-stats.spec.js` exercises the real endpoint and page wiring, verifies that synthetic hidden DOM cannot alter counts, checks cross-tab settings and responsive placement, and verifies the thread remains usable with JavaScript disabled.

The database integration checks IDs above JavaScript's safe-integer range,
deleted replies, transaction visibility, sticky and undead policies, archives and
expiry. Archiving another thread changes the page number and ETag while leaving
the target thread's modification timestamp unchanged. Existing attachment tests
verify the statistics against approved images, file deletion and post deletion.
