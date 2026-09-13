# Native thread updater

Desktop Update links, mobile Update controls and the optional `R` shortcut
fetch and insert new replies in place. Existing drafts, post nodes and document
state remain intact. Automatic updating and Quick Reply are still unfinished;
this is not complete native updater compatibility.

## Public reference

The pinned public `extension.min.1191.js` from September 13, 2026 has SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its `ThreadUpdater.forceUpdate`, `update`, and `onload` functions were inspected
as text, not executed. They fetch thread data, append new replies, reparse the
thread, update state and unread indicators, notify the watcher, and dispatch
`4chanThreadUpdated` with a count. Manual update is not a page reload.
The manual path implements new-reply insertion, status, thread flags, watcher
acknowledgement and the update event. Tail responses, automatic scheduling,
unread indicators, scrolling, sound and Quick Reply coordination remain separate
unfinished behaviors.

## Owned response contract

`GET /_watch/{board}/thread/{id}/posts` returns JSON with `version: 1`, string
`board` and `thread`, boolean `closed`, `archived`, and `sticky`, visible reply
and image counts, and ordered `posts`. Each post has string `no`, boolean
`file_deleted`, and `html`. `HEAD` has matching status/headers and no body.
IDs are canonical positive signed-64-bit decimal strings, never JavaScript
numbers. Query parameters and `.json`/`.html` suffixes are rejected.

The endpoint uses the same read-only repeatable-read database snapshot as the
thread page. The existing model allows at most 1,000 replies; the existing query
returns at most 1,001 visible posts including the OP. It excludes deleted posts
and deleted/expired threads. Counts describe visible posts, not lifetime posts.
The renderer rejects missing OPs, inconsistent board/thread identities, deleted
records, duplicate/out-of-order IDs, and inputs beyond that post bound.

Each HTML fragment uses `post_content.html`, also included by the initial thread
and board-index renderer. Escaped names/subjects, parsed comments, normalized
media URLs, spoiler/deleted-file presentation, and real deletion/report forms
therefore have one rendering implementation. No deletion password, password
hash, asset capability, or private media identifier is added to the projection.
The forms contain empty password inputs, not authorization to submit them.

Rendering has an aggregate byte budget and JSON encoding has a separate
4,194,304-byte final wire budget. Exhausting either produces a non-success
response, never truncated successful JSON. The client independently bounds its
stream/parser and validates the complete response before constructing elements.

This is a public-listener-only, read-only path under the already permitted
`/_watch/` CSP prefix. It adds no script, worker, image, form, CORS, or connection
authority. Responses are `application/json`, `no-store`, `nosniff`, and issue no
cookies. The public thread JSON API and its cache validators are unchanged.
This internal projection deliberately does not add cache validators or tail
responses yet; a client cannot infer a complete result from a partial window.

## Client transport and rendering

The client permits one request at a time with a one-second minimum interval,
a ten-second overall deadline and a two-second parser deadline within it.
Requests omit credentials, reject redirects, use same-origin mode and accept
only the exact configured public URL with a successful JSON response. Actual
streamed bytes must fit 4 MiB regardless of Content-Length. UTF-8 errors,
partial JSON, HTTP failures and oversized responses cannot append posts.

A fresh worker from the fixed native bundle parses JSON and HTML. It terminates
after success, error, timeout or cancellation. No parser fallback runs on the
page. The worker retains the existing network/import/nested-worker denial CSP.
parse5 constructs data without activating HTML. Validation permits only the
shared post renderer's inert elements and finite attributes, exact post IDs,
password-empty public action forms and permitted HTTP(S) links. Images must
use numeric normalized paths on the server-configured media origin. Scripts,
event handlers, arbitrary styles, foreign namespaces, unrelated image URLs and
alternate form actions are rejected. Aggregate node/text and nesting limits
bound both parsing and returned trees.

The page rechecks the returned trees and live DOM IDs before creating elements.
This order matters because even a detached image can fetch after src is set.
It creates elements and text nodes through DOM APIs; response HTML is never
assigned to innerHTML. Only replies newer than the last displayed post append,
in a single fragment. Existing replies are not replaced, so drafts, open menus
and form inputs are preserved. Later snapshots do not reconcile deletion or
file-removal changes in already displayed posts; that integration remains open.

New replies receive existing menus, hiding/filter behavior and watcher read
acknowledgement. After integration, document receives an ordinary,
non-bubbling, non-cancelable `4chanThreadUpdated` event with `detail: { count }`.
No event fires for a response without new replies. Closed/reopened state updates
posting controls without erasing their values. Archived responses and 404 are
terminal. Other failures retain a usable retry. The server continues to
authorize every write independently of these browser controls.

Cross-tab global disabling and page exit cancel pending work; stale responses
cannot append after cancellation. With the extension disabled, mobile controls
retain their ordinary anchored refresh behavior. With JavaScript disabled,
server-rendered navigation and posting remain available. Modifier clicks retain
ordinary link navigation.

Watcher read acknowledgement has a one-second lock deadline after page filters
finish. If another tab keeps the lock, the new replies and update event still
complete, the watcher shows a save warning, and the queued read-position write
is cancelled. Releasing the lock cannot later commit that expired write.

## Coverage and remaining work

Unit coverage exercises exact large IDs, escaping, normal/spoiler/deleted/disabled
media, thread flags, malformed input, UTF-8 byte limits, and JSON-escaping limits.
HTTP coverage checks method/path restrictions, listener separation, headers,
owned persisted posting/deletion, and fragment equality with SSR. The existing
concurrent-commit test includes this response at both connection-release and
in-transaction table-lock barriers.

Eight client unit/HTTP cases cover adjacent large IDs, allowed formatting/media,
malformed and excessive input, unrelated URLs/forms, actual credential-free
streams, redirect denial with a healthy destination control, hung readers,
timeouts, cancellation and worker termination. A seeded 256-case workload
checks that escaped hostile text remains text after entity decoding. Eight persisted browser cases
cover insertion and real reporting, drafts/focus, `R` guards, malformed final
fragments, cancellation/re-enable, 404/retry, filters/hiding, thread states and
contention with an actual cross-tab Web Lock.
The last state-control case supplies synthetic flags through the owned route;
it is not additional staff-authorization evidence. The existing mobile watcher
case now exercises the actual updater while the no-JavaScript case retains real
navigation. A separate synthetic media case checks shared rendering and image
menus after insertion. Six-theme desktop/mobile captures check control fit,
keyboard focus and error visibility; they are not public-page pixel parity.

Still required: automatic controls and `A`, tail/cache behavior, unread/title/icon
and scrolling/sound behavior, Quick Reply coordination, existing-post deletion
reconciliation, remaining native settings and live public-reference comparison.
The full snapshot route and the one-second request floor are explicit local
transport choices. The pending reference qualification remains tracked in #6
and draft PR #89. No production rollout is implied.
