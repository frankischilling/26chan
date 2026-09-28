# Index thread expansion

Board indexes can expand omitted replies in place. The original posts and their
filled forms remain the same DOM objects. Collapse hides the fetched replies;
expanding again reuses them without another request. The ordinary thread link
remains available when JavaScript is disabled or an expansion fails.

The behavior follows `ThreadExpansion` in the released public extension v1191,
pinned by [the watcher asset manifest](public-watcher-assets.json). Its
`toggle` and `fetch` paths insert older omitted replies before the existing tail,
retain them between collapse and expansion, and use the released plus, minus and
loading icons. The local HTML index currently shows three replies. The separate
public JSON preview's five-reply policy is unchanged.

## Snapshot and insertion rules

The control requests the existing full
`/_watch/{board}/thread/{thread}/posts` projection. IDs remain canonical decimal
strings, including adjacent values above JavaScript's safe-integer range. The
existing disposable parser worker validates the entire version 2 snapshot and
the page validates its recipes again before insertion. Tail-only responses,
duplicate or unordered IDs, foreign board identities, unsupported attributes and
unexpected elements reject the whole expansion.

Only replies older than the first displayed reply are inserted. Newer replies
and changes to the existing tail require opening or refreshing the thread page.
This preserves the public client's index-expansion behavior and the user's
unsubmitted form values. The current server sends complete comment bodies, so
this implementation does not invent a truncated-comment transport.

All incoming IDs are checked against the live document before any element is
built. This matters for images, which can begin loading while detached. Fetched
posts keep their real password deletion and report controls. Backlinks, local
dates, media controls, quote previews and filters process them through the same
page callbacks as an ordinary thread update. The owned expansion class is
excluded from quote-source recipes.

## Bounds and lifecycle

A page admits at most 100 expansion controls and one request at a time, with at
least one second between request starts. It retains at most 1,000 fetched posts,
100,000 recipe nodes and 4 MiB of charged text and attributes across all expanded
threads. The existing transport enforces its 4 MiB streamed response limit,
10 second request deadline and two second parser deadline. Stream reads also have
a 65,536-chunk ceiling, including empty chunks.

Pressing the loading control cancels the request. Hidden or offline pages stop
pending work. Disabling the feature removes its added posts and restores the
original summary. Page suspension clears expansion state; browser-history
restoration creates one set of controls. A late response cannot recreate a
cancelled expansion. A failed or stalled integration callback removes the entire
added set, with a separate 10 second application deadline.

The page-only updater and expansion code share the fixed
`native-thread-controls.v1.js` resource, limited to 32 KiB. Its only external
import is the fixed parser/transport resource. The worker remains import-free
and limited to 256 KiB. This split adds no worker, network or media authority.

## Checks

`native-thread-expansion.test.mjs` covers exact large IDs, partial and hostile
snapshots, real worker parsing, retained nodes and drafts, repeated toggling,
shared request admission, cancellation, page-wide budgets, duplicate live IDs,
failed callbacks and history restoration. The persisted browser test creates
eight replies, expands the three-reply index, uses the desktop and mobile
controls, waits for a real filter rule before the expansion event, removes and
restores the added posts, and deletes an earlier fetched reply through its real
form. A separate updater regression verifies that rejected or cancelled page
integration cannot commit thread metadata or leave temporary posts behind.

```text
npm run check:native-thread-controls
npm run test:expansion
```

The [combined verification record](verification-rewrite-completion.md) records
the executed checks, review fixes and remaining compatibility requirements.
