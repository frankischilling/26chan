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
post ID and selected greentext. Ctrl+S wraps the selection in spoiler tags;
Escape or Close removes the dialog and its draft. Switching target threads
clears the comment but retains identity fields. Closed/archived thread state
prevents opening or submitting; updater state changes refresh this guard.

Desktop dragging stores bounded numeric coordinates through the shared
settings lock, never persisted CSS. The default position is right/top 10%;
mobile placement uses the current scroll offset. The dialog uses the current
theme's fields, borders and reply colors. These are rewrite fixture checks,
not a claim of pixel-identical complete original Quick Reply rendering.

Persistent success clears the comment and consumes copied approved-image
capabilities and spoiler controls. Nonpersistent success closes the dialog.
An attached success also removes the ordinary editor's copy of the one-use
capability and spoiler field, makes its comment required and changes its
submit label to Post. Reopening Quick Reply then creates a text-only editor;
the ordinary form's existing comment draft remains intact.
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

Strict JSON receipt parsing retains integer tokens as strings instead of
rounding JavaScript Numbers. It accepts only the expected parent and a later
valid positive i64 post ID. Error envelopes contain one nonempty bounded
string, rendered with textContent. No server-provided markup is executed.

Successful interactive board/thread and upload-result HTML gains only its
current-board posting path in connect-src, alongside the existing read-only
watch paths. Catalog documents and worker scripts gain no posting authority.
This is not a general self-origin fetch permission. Origin checks, stream
limits, rate/admission controls, server deadlines and least-privilege database
roles are unchanged. No database migration or new dependency is required.

The visible deletion password remains the E-011 Argon2 security replacement
for UserPwd identity. Raw files still require isolated upload approval under
E-010; the normal media-board dialog links to that workflow. An approved
reply form can supply its existing capability to Quick Reply. The dedicated
quick-reply-upload.mjs harness runs through synthetic validated output in
the Rust upload browser test and through actual isolated processing in the
native public upload qualification. It checks an image-only spoiler reply,
own-post tracking, a reopened text-only reply, rejected capability replay,
actual normalized-image rendering, file-only deletion and persisted metadata.
The supervising tests retain the one-use tombstone and physical file cleanup
assertions. Existing native/no-JavaScript upload cases still run unchanged.

## Verification and remaining work

- `node --test tests/browser/native-quick-reply.test.mjs`: quote insertion,
  exact IDs, malformed/oversized/ambiguous receipts, request fields, stream
  cancellation and no retries.
- `npm run test:quick-reply`: the transport tests plus persisted browser
  posting, retained failures/drafts, tracking, current-board CSP with healthy
  denied controls, automatic notification suppression and busy-update races.
- `npm run test:media-visual`: six-theme desktop/mobile dialog captures,
  dragging, spoiler caret behavior, closed-thread guards, safe error text,
  aborts and ignored late responses, alongside existing attachment coverage.
- `npm run test:themes` and `npm run test:watcher-core`: existing theme,
  watcher, tracking, keyboard, filter and updater regression coverage.

Local transport and fixture checks have passed. The new persisted-browser
and approved-image pipeline cases await CI because this Windows environment has no available PostgreSQL
service. CI outcomes must be recorded on the tested PR head before merge.

Cooldown/automatic posting, the source comment-byte advisory, identity-cookie
remembering, Pass/captcha, drawing, inline file selection, remaining shortcut
groups and full rendered-source comparison remain unfinished. These are
known source features, not evidence of unknown original behavior. Deployment
boundary qualification and independent launch review remain separate work.
