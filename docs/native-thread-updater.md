# Native thread updater

Desktop/mobile Update and Auto controls, and optional `R`/`A` shortcuts,
fetch and insert new replies in place. Existing drafts, post nodes and document
state remain intact. Automatic updates also maintain the unread title and
last-reply marker, favicon notifications and optional reply sounds. Quick Reply
coordination is still unfinished; this is not complete native updater compatibility.

[Full/tail selection and conditional responses](native-updater-tail.md) follow
the supplied old source. They share the insertion, filtering and notification
path described below.

## Public reference

The pinned public `extension.min.1191.js` from September 13, 2026 has SHA-256
`3d2cd5fbd9fc5266a377f4d7e9c3d10beb438eb9e3ded99433eeb0785abc3f37`.
Its `ThreadUpdater.forceUpdate`, `update`, and `onload` functions were inspected
as text, not executed. They fetch thread data, append new replies, reparse the
thread, update state and unread indicators, notify the watcher, and dispatch
`4chanThreadUpdated` with a count. Manual update is not a page reload.
The manual path implements new-reply insertion, status, thread flags, watcher
acknowledgement and the update event. The same validated transport serves Auto.
Its scheduling and unread rules below come from inspection of `start`, `stop`,
`pulse`, `adjustDelay`, `onVisibilityChange`, `onScroll` and `clearUnread` in that
same file. These are static-source findings, not live public-page observations.

## Automatic scheduling and unread state

Auto begins with a ten-second countdown. Empty results and transient failures
advance through 10, 15, 20, 30, 60, 90, 120, 180, 240 and 300 seconds, remaining
at 300 thereafter. New replies reset the interval to ten seconds in visible tabs
or sixty seconds in hidden tabs. An empty manual update retains the current
interval. Both desktop and both mobile checkboxes reflect the same state;
`A` invokes that control when keyboard shortcuts are enabled.

The per-tab preference uses the native `4chan-auto-{thread}` session key.
Starting writes `1`; stopping removes it. The reference accepts any nonempty
stored value as enabled, including `0`. This is a preference, never authority
to select a request URL. Unavailable session storage leaves in-tab controls
usable. Monitoring settings exposes `threadUpdater` (default true),
`alwaysAutoUpdate` (default false), and `autoScroll` (default false). Always Auto
is an initialization default: a user can stop it for the current document,
and loading the thread again starts it. Global or updater-specific disabling
cancels work and restores the ordinary mobile Refresh link.

Visibility changes reset the displayed countdown to ten seconds. The reference
sets the delay index to four when hidden and below four, and zero otherwise.
The replacement preserves that rule but holds one timer and one active request:
visibility changes during a response cannot create another polling loop.
Request completion owns the next countdown. This bounds request concurrency
and prevents a timer race from multiplying network and parsing work.
Page exit cancels pending work; a persisted `pageshow` can rearm this updater
once. That lifecycle path is tested with synthetic browser events, not claimed
as complete back/forward-cache qualification of every native component.

Automatic insertion on a scrollable page adds to the title's unread count and
marks the previous last reply with `newPostsMarker`. Manual insertion does
neither. A visible scroll to the bottom clears the count and marker; stopping
Auto itself does not clear them. The pinned code only creates a marker when
the previous last post is a reply, and only clears unread when a marker exists.
Consequently the first automatic reply on an OP-only page can leave an unread
title without a marker. This unusual static-source condition is preserved and
tested; its live behavior remains unverified.

The pinned Auto Scroll condition is also unusual: it requires a hidden tab
already exactly at the bottom before insertion. When enabled, that state
scrolls to the new bottom. A visible reader is not moved to new replies.
Changes in the previous last post's offset are compensated after post parsing.
Tests supply explicit hidden/visible states to exercise both paths; they do not
claim browser background-tab timing matches an accelerated test clock.

## Tracked quotes, favicon and sound

On thread pages, ordinary posting receipts feed the existing bounded tracking
store. Quotes to tracked posts receive the native `ql-tracked` class and
` (You)` text suffix. Repeated rendering does not duplicate the suffix or change
the link destination. Disabling the extension removes only its own decoration.
The pinned `Parser.init` loads tracked replies only on thread pages; no new
board-index decoration is inferred. Quick Reply's single-own-reply suppression
still requires the actual Quick Reply lifecycle and is not approximated from
the ordinary posting receipt store.

After automatic insertion, quotes to tracked posts select the reply favicon.
New filter highlights select the highlight icon unless a reply notification is
already active. Ordinary new posts select the new-post icon when unread was
zero. Stop clears the icon without clearing unread text; reading at the visible
bottom clears the icon even in the OP-only marker edge. Archive and 404 select
the dead-thread icon. Manual insertion does not set a new-reply icon.
Insertion observers settle before the explicit filter pass, so notification
priority uses completed filtering for the new posts.

The native `updaterSound` setting defaults to false. Enabling it exposes the two
desktop Sound checkboxes, initially unchecked and synchronized for this page.
Their value is not persisted; the mobile updater does not add a Sound control.
A hidden automatic update that quotes a tracked post attempts playback only
when that control is checked. The client catches synchronous and asynchronous
playback failures so browser autoplay policy cannot break insertion. There is
no fallback audio URL, remote media fetch or notification-permission request.

[The asset manifest](public-updater-assets.json) pins ten unchanged public ICO
files and `https://s.4cdn.org/media/beep.ogg` by URL, length and SHA-256. Names
and priority come from the same pinned v1191 updater. The worksafe default was
observed in captured thread HTML. The general `favicon.ico` was observed on the
public FAQ. The supplied old category settings establish the defaults:
`config/categories/ws.config.ini:11` selects `favicon-ws.ico`, and
`config/categories/nws.config.ini:6` selects `favicon.ico`. Their hashes are
pinned in the [source inventory](compatibility.md#source-inventory). This
resolves the mapping for that checkout; current live-board rendering was not
observed because the catalog request returned 403. That denial was not bypassed.

The application embeds these fixed release assets. ICO routes are added to the
explicit `img-src` list; they do not broaden it to `self` or a directory. The
beep has a separate audio-only GET/HEAD route and is excluded from image sources.
Interactive public pages permit only its exact local path in `media-src`.
Other document and non-script asset responses retain `media-src 'none'`; the
worker's default/network/import denial remains intact. Assets issue no cookies and use nosniff and
revalidation. Upload storage, media-origin authority and API-listener routes
are unchanged.

## Owned response contract

`GET /_watch/{board}/thread/{id}/posts` returns JSON with `version: 2`, string
`board` and `thread`, boolean `closed`, `archived`, and `sticky`, visible reply
and image counts, `tail_size`, nullable string `tail_id`, and ordered `posts`.
Each post has string `no`, boolean
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
authority. Responses are `application/json`, `nosniff`, issue no cookies and
use content-derived validators with mandatory revalidation. Projection version 2
adds bounded tail responses and an exact omitted-boundary ID. The public JSON
API also exposes the source-defined `-tail.json` route; see the
[tail contract](native-updater-tail.md) for counts, eligibility and fallback.

## Client transport and rendering

The client permits one update cycle at a time with a one-second minimum interval,
a ten-second overall deadline and a two-second parser deadline per response.
Requests omit credentials, reject redirects, use same-origin mode and accept
only the exact configured public URL with a successful JSON response. Actual
streamed bytes, including a tail's full fallback, must fit 4 MiB regardless of Content-Length. UTF-8 errors,
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
file-removal changes in already displayed posts, matching the inspected old
updater. Adding reconciliation would be a separate enhancement.

New replies receive existing menus, hiding/filter behavior and watcher read
acknowledgement. After integration, document receives an ordinary,
non-bubbling, non-cancelable `4chanThreadUpdated` event with `detail: { count }`.
No event fires for a response without new replies. Closed/reopened state updates
posting controls without erasing their values. Archived responses and full-response 404 are
terminal; a tail 404 first retries the full response. Other failures retain a usable retry. The server continues to
authorize every write independently of these browser controls.

Cross-tab global/updater disabling and page exit cancel pending work; stale responses
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

Four scheduler tests use bounded virtual time to exercise every interval,
the five-minute ceiling, manual/automatic delay changes, hidden/visible results,
repeated visibility events, stale callbacks, stopping and resumption. Eight
additional persisted browser cases cover the actual countdown/request path,
mirrored controls, `A`, drafts, unread clearing, per-tab storage, persisted
settings, cancellation, terminal archival, unavailable storage, lifecycle
events, scrolling and the OP-only marker edge. Theme captures include Auto,
countdown, error, Monitoring settings and shortcut help at both viewports.

Five notification browser cases exercise actual posting receipts, idempotent
quote decoration, completed filter highlights, favicon priority and terminal
state, real local audio playback, rejected playback, the audio CSP with a
healthy denied audio origin. The existing release-image browser case also decodes all ten icons. Two unit cases
cover priority and fixed-path selection. HTTP tests hash-check release bytes,
GET/HEAD/no-write behavior, MIME, CSP, missing paths and API-listener separation.
Audio tests control the document's hidden flag and confirm playback through the
real HTMLMediaElement API; background-tab autoplay behavior is still subject to
browser policy and requires live-reference qualification.

Still required: Quick Reply coordination, remaining native
settings and live public-reference comparison. The supplied old updater does
not reconcile deletion of already-rendered posts: its `deletionQueue` appears
only at initialization. Adding reconciliation would be a local enhancement,
not a missing original behavior; see the [native source audit](compatibility.md#native-extension).
The full snapshot route and the one-second request floor are explicit local
transport choices. The pending reference qualification remains tracked in #6
and draft PR #89. No production rollout is implied.
