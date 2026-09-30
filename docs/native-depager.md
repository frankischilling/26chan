# Native Depager

Board indexes can load later HTML pages into the current board without replacing
the threads, forms, drafts, or ordinary pagination already on screen. The public
extension v1191 calls this feature `Depager`. Its `alwaysDepage` setting defaults
to false. Desktop exposes an `All` control and mobile changes the next-page action
to `Load More`; when infinite scrolling is enabled, reaching within 350 pixels of
the bottom requests more content.

The behavioral reference is the permitted public
`extension.min.1191.js`, SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
`Depager.init`, `onScroll`, `renderNext`, `toggle`, `enable`, `disable`, and
`depage` are in the formatted copy at lines 315 through 376. The public client
fetches the complete catalog and queues its later pages. This implementation uses
bounded same-origin page snapshots instead. It keeps the visible behavior while
avoiding a growing catalog response and main-thread parsing of fetched HTML.

## Snapshot contract

The transport requests one canonical zero-based HTML page from
`/_watch/{board}/page/{page}`. Board names match `^[a-z0-9]{1,10}$`; page indexes
are integers from 0 through 999 with their canonical decimal spelling. A raw
version 1 response has this shape:

```json
{
  "version": 1,
  "board": "test",
  "page": 1,
  "next_page": 2,
  "threads": [{
    "thread": "123",
    "closed": false,
    "sticky": false,
    "archived": false,
    "replies": 0,
    "images": 0,
    "omitted": 0,
    "posts": [{
      "no": "123",
      "file_deleted": false,
      "html": "<article class=\"postContainer opContainer\" id=\"pc123\"><div class=\"post op\" id=\"p123\"><div class=\"postInfo\" id=\"pi123\"></div><blockquote class=\"postMessage\" id=\"m123\">Owned post</blockquote></div></article>"
    }]
  }]
}
```

`next_page` is exactly `page + 1` or null. A board-page thread is never archived.
The shared board snapshot reads one extra thread-metadata row to decide whether
a successor exists, then discards that row before reading any post bodies. A
full final page therefore does not advertise an empty next page. Direct requests
for empty pages inside the configured page range retain the existing empty-page
response; they contain no successor.
The controller advances only from a validated `next_page`. Transport errors,
including a 404 after the board changes between the rendered page and the later
request, leave the same page available for a manual retry instead of declaring
the board complete.
Each response contains at most 20 threads and 80 posts. The local HTML contract
keeps the OP plus the latest three replies, so `posts.length` is
`min(4, replies + 1)` and `omitted` is the remaining reply count. Post IDs are
canonical positive decimals, globally unique in the snapshot, and strictly
increase inside each thread with the OP first.

Fetched post HTML does not enter the live document or a browser HTML parser on
the main thread. The existing fixed worker parses each post with parse5 and emits
the existing restricted post recipe. `validateBoardPageSnapshot` validates that
parsed response again in the page process. `native-depager.js` performs another
post-tree validation and complete live-ID collision check before constructing any
resource-bearing element.

## Browser controller

`mountNativeDepager` accepts the board root, canonical board/page context, the
current settings reader, and a transport factory. The public bootstrap mounts
`#depage` beside ordinary pagination, with `#depage-status` as the live status
region and `#depage-cancel` while a load is active. Desktop labels the control
`All`: the first click enables page-local automatic loading and fetches the next
page, while a later click can turn that automatic mode off. Mobile labels it
`Load More` and performs a manual next-page fetch. The settings panel separately
exposes `alwaysDepage`, default false. The optional initial `nextPage` comes from
the server-rendered Next link. Passing null marks the page complete without
making a speculative request; omitting it defaults to `page + 1` for isolated
tests.

The returned controller exposes `loadMore`, `refresh`, `cancel`, `stats`, and
`destroy`. `loadMore` is the manual path. `alwaysDepage: true` adds the bounded
scroll and resize trigger; its default false value leaves manual loading
available. `disableAll` disables both paths and removes Depager-owned additions.
The `applied` callback runs after a candidate page is attached so existing
watcher, filter, quote, menu, and image controllers can process its nodes. The
page is not committed to the Depager budget until that callback succeeds. A
throw, deadline, cancellation, page suspension, or teardown removes the complete
candidate. After the callback resolves, the controller also verifies that the
board root is still live and that every candidate marker and thread is still a
connected direct child of it. A hook cannot remove part of a page and still
advance pagination.

Existing thread sections are skipped when the fetched page contains the same
thread ID, matching the public client's behavior. All other thread and post IDs
must be unique. Before `buildPostTree` is called for any new thread, every new
post recipe is validated and checked against every live DOM ID. A collision
rejects the complete candidate before an approved image element can be created.
Existing page nodes are never replaced or cloned, so open details, deletion
password drafts, and other current-page state retain object identity.

The default page marker is an inert `span.depageNumber` containing the one-based
display label, for example `Page 2` for HTML page index 1. Layout styling belongs
to the native layout slice. Ordinary `<nav class="pages">` links stay in the DOM
and continue to work with JavaScript disabled.

## Work limits and lifecycle

One transport request may run at a time. The fixed transport caps one response at
4 MiB and 65,536 stream chunks, strictly decodes UTF-8, rejects redirects and an
unexpected response URL, and gives fetch plus worker parsing ten seconds. The
worker uses the existing 100,000-node and depth-32 post recipe limits. An empty
byte stream is parsed as an invalid snapshot and releases the active request slot.

The controller also applies aggregate limits to additions made during one mount:

| Resource | Limit |
|---|---:|
| Additional pages | 20 |
| New threads | 200 |
| New posts | 1,000 |
| Recipe nodes | 200,000 |
| Serialized recipe text/attributes | 16 MiB |
| Feature-application deadline | 10 seconds |
| Infinite-scroll threshold | 350 pixels |

Reaching an aggregate limit stops further loading and retains pages that were
already committed. A single invalid page never leaves a valid prefix behind.

Hidden or offline documents do not start work. Visibility loss, offline state,
`pagehide`, explicit cancellation, and `disableAll` abort the active transport.
Detached board roots are also ineligible for work and cannot commit an in-flight
page. Cancellation during feature application removes the candidate immediately;
a later resolution or rejection from that callback is ignored. A detached board
also leaves the loading state when its application callback rejects.
`pageshow`, online restoration, and settings changes restore the eligible state;
automatic loading resumes only when `alwaysDepage` is true. BFCache restoration
reuses the existing controller and committed nodes rather than mounting another
copy. `destroy` removes Depager-owned pages, markers, and listeners while leaving
the original board and navigation untouched.

## Verification

`tests/browser/native-depager.test.mjs` checks the exact parsed schema and limits,
the actual fixed parser worker transport, credential omission, current-node and
draft preservation, duplicate-thread skipping, one-flight cancellation, live-ID
collision before image loading, retryable HTTP errors, empty response streams,
late apply completion, detached and partially removed candidates, page-atomic
rollback, BFCache suspension, `disableAll`, the null-next-page case, and
`alwaysDepage` opt-in behavior.

`tests/browser/native-depager.spec.js` is the persisted application integration.
It uses the normal page bootstrap and visible `#depage` controls. A private
fixture creates a board with two threads per page; three posts created through
the public application establish exactly two pages. The test exercises desktop All enable/disable even
after the last page arrives, the live status
and cancel controls, the default-off settings checkbox, mobile Load More, and a
live `4chan_never_show_mobile` change at a mobile-width viewport. That override
must switch the control to desktop All semantics, including turning page-local
automatic loading off after the terminal page has arrived.
The same run checks that existing page nodes, drafts and ordinary pagination
retain identity and that the snapshot request carries no ambient cookie. It also
checks a no-next initial page, rejection of a noncanonical page key and the
JavaScript-disabled pagination fallback on isolated fixture boards, so neither
assertion depends on mutable shared demo content.

These checks establish the bounded local Depager contract. They do not establish
complete original-page visual parity or production deployment qualification.
