# Poster ID controls

Click an ID to highlight loaded posts with the same label. Click it again to
clear that selection, or click another ID to switch. Keyboard users can focus
the label and press Enter or Space. New replies inherit the current selection.
Quote-preview and inline copies do not create additional post counts.

On a thread page, hovering or focusing an ID for 500 milliseconds shows the
number of loaded posts with that label. Index pages offer the count after the
thread is expanded. The count includes loaded hidden posts and omits unloaded
replies; it is not the thread's unique-address counter. Disabling the extension
removes count tooltips. Click highlighting remains available, matching the
separate public core behavior. The ID color preference changes colors only.

## Reference and implementation choices

Public [core v1128](https://s.4cdn.org/js/core.min.1128.js) supplies
`enableClickableIds` and `idClick`. Public
[extension v1191](https://s.4cdn.org/js/extension.min.1191.js) supplies
`Parser.onUIDMouseOver` and `Parser.showUIDCount`. Release hashes and collection
details are recorded in [the control reference](public-identity-controls.json)
and [the existing extension manifest](public-watcher-assets.json).

The rewrite identifies labels through bounded canonical post headers rather
than dynamic `id_` classes. It uses a separate owned highlight class so ID
selection preserves quote highlighting. Counts exclude duplicate projection
copies. These are explicit integration choices; complete original DOM parity
is still unqualified. No request, stored browser preference or authority comes
from clicking a label.

Each scan visits at most 40,000 elements and 10,000 labels. Count tooltips are
omitted if the scan is incomplete. Hover timers stop on blur, pointer exit,
scroll, resize, extension disablement and page suspension. Restoration after
the back-forward cache rescans current posts. Final teardown restores only
attributes and classes the control still owns.

## Verification

`node --test tests/browser/native-poster-id-actions.test.mjs` exercises actual
Chromium with the stylesheet and an explicit CSP. Cases cover click and
keyboard toggles, independent quote highlights, new replies, projected copies,
changed labels, count updates, disabled extension behavior, BFCache, detached
roots, expanded indexes and incomplete scan limits. The persisted case in
`tests/browser/poster-ids.spec.js` checks these controls through the public
handler and updater. Current results belong in the batch verification record.

## Windows failure evidence

Build 36657100871 on `556f0ce` captured a failed
`/static/catalog-preferences.v1.js` request with `net::ERR_NO_BUFFER_SPACE`.
The retained images show the live catalog's unchanged small thumbnails beside
the server's large thumbnails. This establishes a missing-script failure for
that case, not the cause of every older failure in #139 or #142.

Windows socket resource errors can have several causes.
[Microsoft's port-exhaustion guidance](https://learn.microsoft.com/en-us/troubleshoot/windows-client/networking/tcp-ip-port-exhaustion-troubleshooting)
explains how connection-state counts help investigate one of them. The new
diagnostic records TCP-state counts, memory and selected process counts before
browser tests, before theme tests, at a theme failure and after visual tests.
It records no endpoint, process command line, request content or credential.
Unavailable counters are reported as null. It changes no network setting,
retry policy, screenshot assertion or baseline. The runner's resource cause
remains unconfirmed until measured failure evidence is available.
