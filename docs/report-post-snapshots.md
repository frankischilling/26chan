# Report-time post presentation

The source captures public post JSON when admitting a report
(`modes/report.php:546–566,634–642`). Replies also retain their thread's subject.
`reports/ReportQueue.php:855–866` returns that saved presentation to the client.
Reading a current post after submission cannot reproduce it.

Migration 0133 adds immutable public presentation per newly inserted report.
It retains the rewritten post's name, subject, comment, flags, typed formatting,
drawing annotation, thread subject, capture and posting times, archive/file
state and supported attachment metadata. It excludes reporter identifiers,
addresses, credentials, reasons, categories and private author authority.
Old reports are preserved without invented captures.

The uncleared queue renders the selected report's capture while using current
thread flags and current authority for actions. An older row without a capture
uses an explicitly labeled current-data preview. Cleared history renders a
capture when present and states when it is missing; it has no mutation controls.
Janitor reason/category/member/clear-account redaction still applies. Board
scope and final live authorization checks remain enforced.

Saved FP01/FP02 comments use the typed decoder already exercised by text
transfer, including its authority class and source-projection checks. Renderers
escape text and never accept saved HTML. Media links require a currently
available matching media timestamp and format. Captured metadata grants no
permission to reopen removed, retired or expired assets.

## Storage and authority

The capture owner cannot log in, inherit another role or be assumed by runtime
roles. Runtime roles cannot write private captures, capacity or catalog rows.
Staff receives a read-only public projection. Existing public and categorical
admission paths share the private target check, which refuses unhealthy capture
readiness even when its insert trigger is disabled.

Each capture charges its serialized JSON size plus 128 bytes. The initial
storage limit is 64 MiB, with a 32 MiB payload limit; these are resource bounds,
not source retention rules. Exhaustion aborts the entire report admission,
including private membership and anonymous activity. Report deletion cascades
the private capture and releases its charge. Public file-only actions retain it.
No capture is evicted or backfilled to make space. A staff page materializes at
most 16 MiB of capture data and refuses an oversized page explicitly.

The readiness catalog covers the capture relations, views, functions, ACLs and
triggers. An existing optional text-transfer catalog must pass preflight before
the new board permissions are installed. Only the migration identity accepts
that reviewed catalog transition. Prior drift rolls the migration back; later
drift remains rejected. Transfer/image migrations and capture work in either
tested arrival order. Runtime roles receive no resealing or private hashing
permission.

## Verification and limits

The owned migration checker covers legacy preservation, captured data privacy,
unchanged presentation after live edits, public file retention, private cascade
accounting, capacity refusal atomicity, disabled-trigger/ACL drift rejection and
administrator restore with a changed snapshot relation OID. The populated
file/deletion checkers accept only the independently specified new capacity and
qualified catalog rows; they continue comparing all historical data.

Local checks passed 224 domain/staff unit tests and 34 report database/HTTP
tests. These include captured rendering after edits and group clear, scope
denial, Janitor redaction, current action state, read consistency and final
session expiry. One local expiry attempt returned 200 instead of 401. The final
owned run temporarily stopped WSL's NTP service and restored it afterward;
that passing run does not establish the earlier failure's cause. Hosted checks
remain required on the pushed head.

This is a capture of the rewrite's supported public presentation, not every
original JSON/media field. Original-file fidelity, the full source client,
weighted ordering, ignored membership, weighted board counts and unlock remain
open under #219 and the media parity issues.
