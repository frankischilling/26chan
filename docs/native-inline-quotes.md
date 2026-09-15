# Native inline quotes

## Implemented behavior

This slice implements the public client's optional inline quote expansion. It is
disabled by default. A qualifying quote click opens the referenced post beside
the source; activating the same source again closes it. The original destination
and its mobile navigation companion remain usable. Inline activation precedes
mobile preview click handling, while disabled inline quotes retain the existing
preview behavior.

The feature uses the existing one-post endpoint and two fixed release modules.
It adds no route, database role, dependency or CSP grant. Final browser and CI
qualification is recorded below and in the implementing PR.

## Permitted reference

[public-inline-quote-reference.json](public-inline-quote-reference.json) records
the already pinned public extension v1191, its digest and the relevant source
offsets. The code was inspected as text. Its local and remote placement, loading,
toggle, nesting and modifier branches establish the feature contract; no private
backend implementation or production post was consulted.

The source's default is false. Its desktop settings label is **Inline quote
links**; mobile settings omit the checkbox, but an existing saved value still
applies. A primary quote activation is handled before the mobile preview branch.
Shift follows the ordinary destination. The source does not exempt Control,
Meta or Alt from inline activation. Direct self-quotes retain navigation; other
ancestor cycles must not create recursive copies.

Ordinary local expansion follows the anchor or its immediate quote wrapper,
outside consecutive spoiler wrappers. Desktop backlinks have a separate path:
the copy is prepended into the backlink owner's message, and the original post is
hidden until all corresponding owned copies close. Mobile backlinks use adjacent
placement. Local copies omit already expanded descendants. Loading and error
states remain observable and retryable.

## Boundaries and integration

The rewrite constructs each copy from the existing validated finite post
recipe. Forms, filled controls, duplicate IDs, unsafe resources and interactive
staff or posting controls do not travel into the projection. The same-origin
single-post endpoint replaces the source's external full-thread request/cache.
It cannot distinguish a missing thread from a missing post, so unavailable
responses say that the post or thread is unavailable. Loading, unavailable and
other errors remain attached to the initiating source. A pending click does not
restart its request; a later click dismisses a completed or failed copy.

Every displayed copy belongs to one controller record. A single page-created
`createCommentProjection()` registry is injected into the cooperating readers.
Canonical post, receipt, filter, filename, watcher, updater and backlink readers retain the
original inputs, even while nested copies are visible. A matching CSS class alone
does not authorize exclusion from those readers. Original quote annotations and
literal text remain distinguishable, and copied backlink rows cannot register
new graph edges. The registry also removes only the current controller's owned
presentation attributes when serializing original comments. Reader failures at
the existing comment bounds leave originals available and do not tear down the
watcher.

Admission charges a complete inert build plan before creating copied elements or
assigning resources. The limits are fixed maxima; test overrides may only lower
them.

| Resource | Limit |
|---|---:|
| Open inline copies, including loading/error placeholders | 16 |
| Pending requests, including queued work | 8 |
| Active inline transport requests | 1, FIFO |
| Nested copies | 8 |
| Aggregate copied nodes | 32,768 |
| Aggregate copied text and attribute characters | 524,288 UTF-16 code units |
| Per-post copied nodes | 16,384 |
| Per-post copied text and attribute characters | 262,144 UTF-16 code units |
| Mobile quote companions in one copy | 512 |
| Pending deadline, including time in the queue | 15 seconds |

The existing response-byte, parser-depth, transport and worker deadlines also
apply. Overflow before admission retains ordinary link navigation. Collapse,
source invalidation, settings changes and page lifecycle cleanup cancel work and
release all descendant effects. Late responses cannot recreate removed copies.
Desktop backlink copies reference-count only their own original-target hiding;
the last collapse preserves independent manual and filter hiding.

The quote-feature page module remains at `/static/native-backlinks.v1.js`, with
its existing 32,768-byte ceiling. It imports no parser or network code. The
existing `/static/native-filter.v1.js` exposes the finite preview helpers and
keeps its 262,144-byte ceiling. Reproducible build checks enforce exact entry
points, imports and exports. Current outputs are 29,260 and 261,279 bytes.

## Mobile input arbitration

The public client handles compatibility mouseover before its click branch; it
does not globally suppress mobile hover. In the rewrite, a touch-generated
compatibility mouseover on an exact quote anchor may defer a remote preview
when the inline controller reports that the ensuing activation is eligible.
This avoids spending two requests on one tap. The predicate checks current
source ownership, identities, settings and admission capacity without mutating
state or starting work. It is consulted only when the event reports touch
capability. Ordinary hardware-mouse hover, child targets, local targets,
disabled inline quotes and missing capability information keep the existing
preview path. The inline click remains the final admission decision.

This is a bounded request-arbitration replacement, not evidence that the public
client omitted hover handling. The real mobile-tap regression first observed a
preview request followed by an inline request. With the page wiring and fixed
release modules, that same tap sends one inline request; disabling inline quotes
still opens the retained mobile preview and leaves its `#` companion usable.

Mobile settings construct only the layout's eligible controls. They omit Inline
quote links and Pin Thread Watcher rather than creating hidden checkbox fields.
Saving another mobile setting does not overwrite the existing inline preference.

## Verification

`npm run test:inline-quotes-core` passes 25 cases. They cover exact ownership,
finite recipes, atomic admission, aggregate character/node accounting, real
disposable parser workers, recursive cleanup, settings failures, original-input
projection and hostile resources. Its isolated DOM cases distinguish trusted
mouse/touch input from synthetic lifecycle and negative controls.
`npm run test:quote-preview-core` passes 40 preview, transport and filter-settlement
cases with the new shared helpers and input arbitration.

The persisted browser suite has 36 cases using the actual public server, owned
posts, real settings and the bounded post endpoint. Held-response cases preserve
the real body and alter only delivery timing. Adversarial substitutions and
augmented DOM are labeled separately. The updater case holds a real filter job
while a copy is visible and verifies original filter input, notification priority
and watch acknowledgement after settlement. Six theme cases exercise desktop
and mobile widths without changing visual baselines.

Fixture corrections retain the behavior under test. The persisted spoiler case
uses the single wrapper that its board actually renders; consecutive nested
spoilers remain covered by an isolated DOM control. Cap fixtures use distinct
lines with the same quote target, so the real repeated-line spam check accepts
the posts before inline admission is exercised. Watcher assertions wait for the
actual saved row. Navigation cancellation records the actual fetch AbortSignal
synchronously in a bounded session-storage witness that survives document
replacement, rather than depending on a browser-protocol failure event for a
retired page.

The final combined browser run, Rust asset/security checks and complete CI must
pass before merge. These tests do not establish full original-page parity or
production readiness.
