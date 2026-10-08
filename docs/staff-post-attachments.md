# Staff attachment posting boundary

The published 0110 backend adds proof-bound attachment consumption to ordinary
and badged staff posts. Its recorded local qualification is listed below.
The 0111 continuation adds development-only HTTP upload, status and cancellation
routes and a native form workflow. Hosted qualification is still pending;
its presence does not establish a passing browser workflow or full source parity.
Production media remains disabled.

## Reference and scope

The reference is the supplied `545b7812d1849f7958d914950c91fdbbe38f6b22` checkout.
In `imgboard.php`, lines 4866–4867 retain text-only reply restrictions;
4987–4991 reject archived replies and permit moderator closed-thread exceptions;
5045–5051 apply the image-count exception by staff level, independently of the
visible badge. Lines 5066–5069 retain upload limits, 5426–5428 apply spoiler
policy, and 5732–5733 preserve filename admission for ordinary posts and the
source's test-board branch.

The rewrite uses its existing approved, normalized development-media receipts.
Its 8 MiB intake and 5 MiB published-output bounds remain enforced for staff.
These bounds and normalized outputs do not establish original-file, decoder,
per-board byte-limit or duplicate-image parity. Ordinary staff duplicate-image
admission remains a separate gap.

## Development configuration

Without staff media settings, the application keeps its text-only posting path.
Enabling the development upload path requires all three settings together:

- `STAFF_MEDIA_PROFILE=isolated-development`
- `STAFF_INTAKE_ADDR`: a numeric loopback socket address with a nonzero port
- `STAFF_INTAKE_TOKEN`: a 64-character lowercase hexadecimal service credential

Partial settings, an invalid endpoint or enabling media in `STAFF_MODE=production`
fail configuration. The existing public, staff and media origin separation still
applies. Do not put public-client, intake-server or media-database credentials in
the staff process environment.

The intake service has an optional `MEDIA_INTAKE_STAFF_TOKEN` slot. Set the staff
client token to that independent value, not to `PUBLIC_INTAKE_TOKEN`. The intake
service rejects duplicate public, staff and metrics credentials. Omitting the
staff slot preserves public-only intake authentication. Both client credentials
use the same bounded intake operations; the staff credential does not convey a
staff session, board permission or database authority. See
[HTTP intake](media-intake.md) for the service configuration.

## Upload and receipt workflow

The upload form posts one file to `/post/upload`. The returned page offers native
POST forms for `/post/upload/status` and `/post/upload/cancel`. Status checks are
manual. Once intake reports an approved published output, the page exposes the
ordinary or badged posting form, with an optional image spoiler. Receipt pages
load no scripts, and this flow is designed to work with JavaScript disabled.
Upload first, then enter the post text; this is not a saved-draft editor.

Every upload, status and cancellation request checks the staff origin and
same-origin Fetch Metadata, a live session, CSRF, recent authentication, rank and
board access. `/j/` is excluded. Upload and status also perform preliminary board
and thread checks. The final post transaction still decides whether the receipt
and target are admissible. Cancellation remains available if a thread closes or
fills after upload, provided the caller still has current staff authority and
board access.

Receipts contain an upload ID and a bearer capability. They are carried in hidden
POST fields, not query strings, logs or redirects, and pages are private/no-store.
They are not session-owned, account-owned or cryptographically board-bound drafts.
Anyone who obtains an unspent receipt can use its existing bearer authority
through an otherwise authorized path. Keep receipt HTML private. The database
rejects malformed, expired, revoked or already attached receipts; the existing
two-hour receipt window is unchanged.

## Streaming and cancellation

The staff route limits the file to 8 MiB and multipart overhead to 16 KiB. It
accepts only the expected metadata fields followed by one file. A finite control
envelope is parsed before requesting further file frames, then replayed through
the same multipart parser after fresh authorization. An already-delivered frame
may contain both controls and file bytes; it remains bounded by the total request
cap. The parser adapter yields between pieces to avoid draining the ready file
stream into its buffer. The producer and HTTP upload run together without a
detached task, using a one-slot channel and at most 16 KiB per queued piece. The shared client uses a fixed numeric endpoint,
does not follow redirects or resolve arbitrary URLs, and bounds response JSON to
4 KiB. Connect, exchange and route deadlines bound slow requests. No media decoder
runs in the staff process. Malformed file headers may accumulate up to the total
request cap before rejection; this is not a universal 16 KiB memory bound.

On a detected failure after receiving a reservation, the route attempts bounded
receipt revocation. Cleanup is best effort: disconnects, an uncertain reservation
response or failed cleanup can leave intake expiry and worker cleanup as the
fallback. Revocation deletes the bearer handle, not the job, a worker lease or a
published file. An already running worker may finish; its output cannot attach
using the revoked receipt.

Explicit cancellation first takes the media job lock through the reviewed
cancellation function in a staff transaction, then obtains fresh account/session
authority. A failed recheck rolls back the tentative cancellation. This follows
the media-before-account order used by final attachment insertion and avoids
holding account locks over an intake network exchange.

## Authority and transaction

Existing staff text APIs remain wrappers with no attachment. New ordinary and
source-identity variants accept an optional `NewAttachment`. They still insert
posts directly as `board_staff`, so the existing identity and authorization
triggers run. Staff never receive the public attachment insertion function or
raw media-table access. Migration 0111 adds only staff EXECUTE grants on the
existing `content.check_attachment_upload(text,text)` and
`content.cancel_attachment_upload(text,text)` functions. Their public grants,
reviewed definitions and 0110 owner-only receipt helpers remain unchanged.

The authentication pool issues a short-lived proof binding the post, identity,
board and existing policy fields, plus the media job, a capability hash and the
initial spoiler choice. Only complete attachment tuples are accepted. Text
proofs retain null attachment fields, including pending pre-upgrade proofs.
Plaintext receipts are not stored in the proof table.

An owner-only helper locks board, thread and media job before account/session
and proof locks. Filename admission and direct SQL insertion therefore follow
the same order. Receipt state, capability and wall-clock expiry are checked
after waiting. Moderator exceptions derive from verified authority, not a badge
string or caller-controlled transaction setting.

Proof consumption creates a private handoff for that new post in that same
transaction. The post's AFTER trigger consumes it and attaches only an approved,
unspent receipt. A second proof cannot reuse a post with a pending handoff.
A deferred constraint rejects a handoff left without its insertion. Runtime
roles cannot attach a receipt to an arbitrary existing post.

The final step rechecks authorization deadlines after media waits. Failure
rolls back the post, receipt, audit, history and counters together. Existing
media-number defaults and spoiler synchronization remain active. The media owner
has no staff identity or proof-table privileges.

## Published 0110 backend qualification

Nine restricted-role integration cases cover ordinary and source identities,
rank and board rules, filename admission, one-use receipts, proof substitution,
forged settings, orphan rejection, rollback, revocation, deadline checks behind
real locks and cancellation. They serialize the shared media-queue fixture while running
deliberate concurrent requests inside the race cases. A two-board case exercises ordinary and badged
requests using the same account and receipt without an account/media lock cycle.
The expiry probes use a bounded transaction-local lock timeout; they separately
assert that the normal staff defaults remain two seconds for locks and five
seconds for statements.

`test-staff-post-attachment-migration.sh` checks fresh installation and a populated
0109 upgrade, retained rows and grants, migration rollback, pending text-proof
compatibility and rejected private-contract drift. Historical assertions run at
0110. The continuation adds a separate 0111 checkpoint before later migrations
and the current readiness query. Readiness inspects catalogs rather than reading
private handoff or identity rows.

Local qualification passed 235 store checks with eight test threads, including
all nine new attachment cases. Three additional eight-case automatic-admission
runs passed after isolating the expiry observation from unrelated mint cleanup.
The 153 supported staff checks, 127 supported public library checks, three public
poll/upload integration cases and 118 domain checks passed. Two staff Unix-peer
cases and one public Unix-peer case were excluded only in the local executor;
they remain enabled in CI. Staff CLI binaries used a local ICU 74.2 search path
for subprocesses that intentionally clear their environment.

Fresh/populated migration qualification and the complete role-bootstrap script
passed against PostgreSQL 16.15. The local adaptation uses an owned loopback TCP
cluster; CI runs the unchanged privileged Unix-socket scripts. Rust formatting
and strict all-target, all-feature workspace Clippy passed for that backend
slice. Those results do not qualify the later HTTP/client/UI changes.

## 0111 continuation qualification

The final local staff run passed 178 checks, including all 18 upload HTTP cases
and six form/parser/streaming unit cases. Two Unix-peer cases were excluded only
locally; they remain enabled in CI. The script-disabled browser case was not run
in the local executor. Selected public regressions passed 130 checks, and all
118 domain checks passed. The shared client passed six tests; intake library and
configuration suites passed five each. Strict workspace Clippy and formatting
passed after the final changes.

`scripts/verify.sh` explicitly invokes the ignored `script_disabled_browser` case
in addition to the normal suite. Wiring that invocation does not establish that
browser qualification has passed.

New database checks cover exact staff/public control grants, denied raw media
and owner-role access, malformed and bad capabilities, used and expired receipts,
and cancellation preserving running leases and published output. The migration
script checks 0111 rollback and retained definitions, rows and ACLs, allowing only
the two intended EXECUTE additions. Readiness must reject missing or excessive
control grants, grant options, altered search paths and raw media-table access.
The full store run passed 235 checks, including the new controls cases, plus
24 repeated concurrent admission checks. Fresh/populated migration checks and
complete role bootstrap passed on PostgreSQL 16.15. Exact-head hosted CI and the browser case
are still required before this continuation can be considered qualified.

Original-page visual parity, duplicate-image admission and production media
qualification remain separate work. This continuation makes no full parity or
production-readiness claim.
