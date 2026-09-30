# Native Quick Reply

Quick Reply opens from a post number, the desktop thread's Post a Reply link,
or the optional Q shortcut. The quickReply preference defaults to true;
persistentQR defaults to false. Posting uses the real
[multipart JSON handler](posting-multipart.md), not a navigation substitute.
The server remains responsible for limits, thread state, posting transactions
and attachment authority.

## Source and implemented scope

The supplied, pinned old source was inspected as text. In `js/extension.js`,
QR.addReplyLink (3793), quotePost/addQuote (4000/4010), show (4050),
onKeyDown (4337), close (4396), submit (4531) and the updater's lastReplyId
check (6567) establish the implemented lifecycle. The defaults are at
8798/8835 and settings at 8973-8974. See the
[source inventory](compatibility.md#native-extension) and [issue #100](https://github.com/frankischilling/26chan/issues/100).

The dialog edits name, options and comment without replacing the ordinary
form draft. Quote insertion replaces the textarea selection with the exact
post ID and selected greentext. Ctrl-click omits the post ID, even when optional
keyboard shortcuts are disabled. Q works only on thread pages and inserts the
current selection as greentext without a post ID. Post a Reply opens and focuses
the editor without inserting the page selection. The quote entry points follow
`js/extension.js:4000-4046,8251-8254,9999-10003`; a closed-thread quote displays
`This thread is closed` without following the post-number link.
Ctrl+S wraps the selection in spoiler tags;
Escape or Close removes the dialog and its draft. Switching target threads
clears the comment but retains identity fields. Closed/archived thread state
prevents opening or submitting; updater state changes refresh this guard.
Initially closed thread pages [omit ordinary posting forms](native-posting-visibility.md)
while retaining the quote alert. Quick Reply cannot create a posting editor
without a server-rendered source form.

On media-enabled board and thread pages, Quick Reply also exposes the source
file control as `#qrFile`, with the source's accepted image types and spoiler
checkbox. The pinned public v1191 extension asset uses the same visible file
selector and `#qrSpoiler` identifiers, including Shift-click removal. This
rewrite additionally accepts a single dropped File on that row; drop support is
a rewrite convenience, not a claim about the pinned source. Choosing another
file replaces an existing QR upload only after its known capability is revoked.
If revocation fails, the existing approval stays in place and the replacement
does not begin.

Ordinary and approved forms expose the board's max_comment_chars as an escaped
data-comment-limit. As in `imgboard.php:3407/3434` and
`js/extension.js:4175-4178,4337-4379`, keydown, paste and cut schedule one
comment check after 500 ms.
It counts UTF-8 bytes and shows `Error: Comment too long (bytes/limit).` with
data-type=length. A shorter comment clears only a length warning, preserving
unrelated server errors. Closing cancels the pending check. The advisory does
not disable posting: the source server separately counts normalized characters
at `imgboard.php:5295-5302`, and the rewrite retains that distinction. A
multibyte comment may therefore warn yet post successfully.

Desktop dragging stores bounded numeric coordinates through the shared
settings lock, never persisted CSS. The default position is right/top 10%;
mobile placement uses the current scroll offset plus 28 pixels initially and
25 when reopening the existing dialog. The qrResto target ID and QR-position
data-trackpos marker follow the source; position storage remains numeric.
Quote insertion scrolls to the bottom only when its caret reaches the end.
The dialog uses the current
theme's fields, borders and reply colors. These are rewrite fixture checks,
not a claim of pixel-identical complete original Quick Reply rendering.

Persistent success clears the comment and consumes copied approved-image
capabilities and spoiler controls. Nonpersistent success closes the dialog.
An attached success also removes the ordinary editor's copy of the one-use
capability and spoiler field, makes its comment required and changes its
submit label to Post. Reopening Quick Reply then creates a text-only editor;
the ordinary form's existing comment draft remains intact.
An inline file follows the same one-use rule. The posting form receives
`upload_id` and `upload_capability` only after status reaches `approved`; queued,
processing, failed and incomplete uploads cannot be submitted. Persistent
success resets the file state and disables the reusable spoiler checkbox while
leaving Quick Reply open for another post.
Both paths record the committed reply, consume posting receipts, and emit
4chanQRPostSuccess with exact string threadId/postId values. The updater
requests a snapshot after 500 ms, or after its in-flight request finishes.
An automatic update suppresses unread/icon/sound changes only if its sole
addition is the last Quick Reply post, as in the source. Multiple additions
use the normal notification path.

## Security and transport

The request has one fixed current-board imgboard.php destination, exact
Accept: application/json, same-origin credentials, multipart text fields,
no redirect following and no automatic retry. The client limits submitted
text to 90,000 UTF-8 bytes, responses to 8,192 bytes, and the complete request
to 15 seconds. Closing, a second submit, page suspension or global feature
disable aborts the current request. Late responses cannot clear a replacement
dialog or draft. Cancellation cannot roll back a committed server transaction;
ambiguous failures tell the user to check the thread before posting again.

Inline media uses only the existing current-board `/upload`, `/upload/status`
and `/upload/cancel` endpoints. The first request contains exactly one file
with `resto` before `upfile`; the client rejects empty files and files above
8 MiB before sending them. Responses are streamed through a 4,096-byte ceiling
with fatal UTF-8 decoding and exact JSON schemas. Upload receipts retain the
32-hex ID, 64-hex capability and canonical string parent. A status response
must return the same ID and capability it was asked about. Only exact bounded
JSON error messages from the server are shown; transport, reader and non-JSON
failures collapse to fixed client text.

Automatic status checks use finite 1, 2 and 4 second backoff. If processing is
still pending, Quick Reply stops polling and exposes an explicit Check status
button. Cancel remains available while an upload or status request is active.
Before a receipt exists it aborts the request; after a receipt exists it first
aborts the active check and then revokes that exact capability. Close, feature
disable, target replacement and page suspension invalidate the local generation
so late upload or approval results cannot attach to a reopened editor.
Reopening starts with an empty file status and disabled spoiler control. During
a posting attempt, file replacement, status checks and upload cancellation are
disabled until a definite server error or the final posting result arrives.

Once an approved capability enters a posting attempt, an ambiguous posting
result is never automatically canceled or reused. Quick Reply removes that
authority and tells the user to inspect the thread. A normal server JSON posting
error leaves the approval in place so the user can correct the post and try
again.

Strict JSON receipt parsing retains integer tokens as strings instead of
rounding JavaScript Numbers. It accepts only the expected parent and a later
valid positive i64 post ID. Error envelopes contain one nonempty bounded
string, rendered with textContent. No server-provided markup is executed.

Successful interactive board/thread and upload-result HTML gains only its
current-board posting and upload paths in connect-src, alongside the existing
read-only watch paths. Catalog documents and worker scripts gain no upload or
posting authority.
This is not a general self-origin fetch permission. Origin checks, stream
limits, rate/admission controls, server deadlines and least-privilege database
roles are unchanged. No database migration or new dependency is required.

The visible deletion password remains the E-011 Argon2 security replacement
for UserPwd identity. Raw files still require isolated upload approval under
E-010; the normal media-board dialog and inline Quick Reply both use that
workflow. An approved reply form can still supply its existing capability to
Quick Reply. The page-only Quick Reply module is built deterministically as
`/static/native-quick-reply.v1.js`; keeping it separate leaves the
`native-filter.v1.js` worker import-free and under its 256 KiB ceiling.

The dedicated `quick-reply-upload.mjs` harness retains the approved-confirmation
scenario and adds an inline-selector mode. The Rust upload-browser supervisor
drives both through real public HTTP and intake with synthetic trusted
publication. The existing native media qualification invokes the inline mode
through its non-root public browser and real isolated guest pipeline. Both
scenarios check an image-only spoiler reply,
own-post tracking, a reopened text-only reply, rejected capability replay,
actual normalized-image rendering, file-only deletion and persisted metadata.
The supervising tests retain the one-use tombstone and physical file cleanup
assertions. Existing native/no-JavaScript upload cases still run unchanged.

## Verification and remaining work

- `node --test tests/browser/native-quick-reply.test.mjs`: quote insertion,
  exact IDs, upload/status/cancel receipt binding, bounded response streams,
  caller cancellation, private transport errors, request fields and no retries.
- `npm run check:native-quick-reply`: deterministic page-module inputs, fixed
  watcher-core imports and the 32 KiB release budget. The worker bundle retains
  its separate 256 KiB check.
- `npm run test:quick-reply`: the transport tests plus persisted browser
  posting, retained failures/drafts, tracking, current-board CSP with healthy
  denied controls, automatic notification suppression and busy-update races.
- `npm run test:media-visual`: six-theme desktop/mobile dialog captures,
  dragging, spoiler caret behavior, closed-thread guards, safe error text,
  inline selection/drop/replacement, finite status polling, cancellation,
  aborts, ignored late responses, debounced byte advice, typed-error preservation
  and source editing positions, alongside existing attachment coverage. Q's
  thread-only selected-text behavior, Ctrl-click without optional keybindings
  and closed-thread alerts have dedicated interaction cases.
- `npm run test:themes` and `npm run test:watcher-core`: existing theme,
  watcher, tracking, keyboard, filter and updater regression coverage.

Eleven transport cases, both deterministic bundle checks and all 23 Windows
Quick Reply browser cases passed. Two added regressions first reproduced stale
upload state after reopening and mutable attachment controls during posting;
both pass with the lifecycle fixes. A separate disable/re-enable fixture now
dispatches the same settings event in both directions. No retries or screenshot
tolerances changed. The local WSL host has `/dev/kvm`, but does not
currently have the owned Firecracker qualification configs or Linux candidate
binaries. Hosted media/operations run 36643399587 passed the real-guest inline
case on df2d20e. The local persisted supervisor then passed all six upload,
post and deletion workflows with the response-body capture fix, including the
subsequent tripcode integration. These checkpoints do not replace CI for the
final PR head.

Cooldown/automatic posting, identity-cookie
remembering, Pass/captcha, drawing and full rendered-source
comparison remain unfinished. The help lists the source's Global and built-in
Quick Reply shortcut groups; exact help geometry remains unqualified. These are
known source features, not evidence of unknown original behavior. Deployment
boundary qualification and independent launch review remain separate work.
