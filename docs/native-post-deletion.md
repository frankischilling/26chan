# Mobile post and file deletion

The mobile post menu now asks `Delete post?` or `Delete file?` and completes
an authorized deletion in place. It follows `Del.deletePost` and
`Del.onPostDeleted` in the supplied `js/extension.js:8358-8416` at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Successful post deletion dims the
original container; file-only deletion dims its original thumbnail while
retaining the comment. Neither action navigates or changes an open draft.

The existing same-origin legacy route receives `mode=usrdel`, the exact string
post ID as a field name with value `delete`, and `onlyimgdel=on` when requested.
The browser sends its anonymous session cookie. It does not send a tracking
receipt, invent ownership, or retrieve a saved recovery password. Ordinary
script-free deletion forms remain available. The server's existing board,
authority, age, quota and protected-post checks still decide whether deletion
is allowed. Archived deletion is not added by this client change.

## Bounded completion

Each request has a ten-second deadline and a four-KiB response ceiling. At most
four requests run concurrently. Redirects and responses other than the complete
fixed current-board success document are rejected. Returned HTML is never
inserted or executed. This replaces the reference's loose success-text search.

Repeated clicks and fallback-form submissions cannot duplicate a pending or
uncertain request. Disabling the extension, suspending the page, or replacing
its original target fences delayed results. A timeout or incomplete response
leaves the target unmarked and asks the user to refresh before trying again;
the client does not infer that the server rolled back or retry automatically.

Completion closes the target's open menu. The image controller retires exact
validated media URLs so existing inline quotes and previews cannot reopen a
deleted file through expansion, hover, filename links or middle-click. Its
bounded retirement set never evicts an entry to make a deleted URL usable again.
Other files remain available. These guards do not replace server-side media
revocation.

The never-mobile preference now controls both menu actions and mobile menu
styling. Desktop and archived targets do not receive mobile deletion controls.

## Qualification

Local transport and image-core tests passed 12 cases. The existing watcher
regressions passed all 167 cases. Generated bundles are checked against pinned
build inputs, and the Windows grouped-command exit checker passed. Strict all-target/all-feature workspace Clippy passed. The public library
passed 128 cases with one unsupported Unix-socket test excluded. Ten asset
routing cases and ten real-database authorization, quota, batch and legacy
deletion cases passed. Initial local attempts lacked fixture role variables
and the synthetic board seed; reruns with those prerequisites passed.

The browser tests cover actual owned OP/reply deletion, another session's
rejection, cancellation, repeated clicks, held real responses and layout gates.
The isolated-upload test's inline branch exercises actual file-only deletion,
retained comment data, revoked media access and reload presentation; its
non-inline branch retains ordinary-form coverage. DOM tests cover original-node
ownership, replacement and suspension races, and copied media retirement.

These browser/DOM scenarios have not run locally: Chromium is unavailable or
cannot create the required sockets in this executor. Discovery and syntax
checks are not browser passes. Exact-head hosted qualification is required.
This slice does not close all deletion policy, physical-erasure or native-client
parity gaps in #214 and #218.
