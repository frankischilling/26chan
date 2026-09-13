# Native thread updater: snapshot groundwork

The updater UI and the `R`/`A` shortcuts are not enabled by this slice.
This establishes a release-owned rendering response for the future in-place
updater. It does not claim complete native updater or Quick Reply compatibility.

## Public reference

The pinned public `extension.min.1191.js` from September 13, 2026 has SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its `ThreadUpdater.forceUpdate`, `update`, and `onload` functions were inspected
as text, not executed. They fetch thread data, append new replies, reparse the
thread, update state and unread indicators, notify the watcher, and dispatch
`4chanThreadUpdated` with a count. Manual update is not a page reload.
Tail validation, automatic scheduling, scrolling, status, sound, and Quick Reply
coordination are separate behaviors, not implemented by this response.

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
response, never truncated successful JSON. A future client must bound its own
stream/parser, reject incomplete/non-success responses, validate exact IDs and
owned markup/URLs, and make DOM changes only after complete validation.

This is a public-listener-only, read-only path under the already permitted
`/_watch/` CSP prefix. It adds no script, worker, image, form, CORS, or connection
authority. Responses are `application/json`, `no-store`, `nosniff`, and issue no
cookies. The public thread JSON API and its cache validators are unchanged.
This internal projection deliberately does not add cache validators or tail
responses yet; a client cannot infer a complete result from a partial window.

## Coverage and remaining work

Unit coverage exercises exact large IDs, escaping, normal/spoiler/deleted/disabled
media, thread flags, malformed input, UTF-8 byte limits, and JSON-escaping limits.
HTTP coverage checks method/path restrictions, listener separation, headers,
owned persisted posting/deletion, and fragment equality with SSR. The existing
concurrent-commit test includes this response at both connection-release and
in-transaction table-lock barriers.

Still required: bounded client transport and parsing, new-reply insertion,
native update events and extension integration, manual/automatic controls and
shortcuts, cancellation/stale-response handling, unread/scroll/state behavior,
owned browser workflows, and live public-reference comparison. The available
browser-control tool currently has no connected browser, so live reference
captures remain outstanding. No production rollout is implied.
