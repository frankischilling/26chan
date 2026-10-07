# Staff attachment posting boundary

Migration 0110 and the store APIs add attachment consumption to authenticated
ordinary and badged staff posts. This is a database/store foundation. The staff
HTTP intake routes, upload form and browser workflow are not implemented by this
slice, and production media remains disabled under the existing policy.

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

## Authority and transaction

Existing staff text APIs remain wrappers with no attachment. New ordinary and
source-identity variants accept an optional `NewAttachment`. They still insert
posts directly as `board_staff`, so the existing identity and authorization
triggers run. Staff never receive the public attachment insertion function or
raw media-table access.

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

## Qualification

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
0110; later migrations run before the current readiness query. Readiness inspects
catalogs rather than reading private handoff or identity rows.

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
and strict all-target, all-feature workspace Clippy passed. These local results
still require exact-commit hosted CI before merge. HTTP attachment handling and
original-page presentation need their own implementation and tests afterward.
