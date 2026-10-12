# Staff self-deletion

Migration 0140 adds the distinction in `imgboard.php:2490–2497,2545–2594`:
when an authorized staff member supplies the post's password or matches its
posting peer, a file-only deletion keeps the reports and records a self-deletion
event. Ordinary administrative file deletion still clears the target's reports.
Account authorship alone does not establish either proof.

All requests still require the staff session, recent authentication, CSRF and
origin checks, board permissions and the applicable sticky-post rank. The proof
changes report handling and audit classification; it grants no staff permissions.
Whole-post and whole-thread deletion retain canonical content erasure and remove
the affected reports, including when the request is classified as self-deletion.
The audit actions are `self-remove-file`, `self-remove-post` and
`self-remove-thread`. Existing administrative action names remain unchanged.

## Proof and security differences

The peer check compares the verified current transport peer's keyed digest with
the target's existing private posting history. It never accepts an address or
digest from the form, cookies or untrusted forwarding headers. Public deletion
continues requiring its existing session capability or password; sharing a
network does not grant anonymous deletion authority.

The staff form also accepts an optional recovery password. It uses the existing
bounded Argon2 profile and worker limit. Staff and public cookies remain isolated
to their respective origins. This explicit field is a UI difference from the
source's password cookie. A supplied password that no longer verifies, without
a matching current peer, refuses the request rather than silently treating it as
an administrative deletion and clearing reports.

A missing historical peer record, absent key or rotated key does not create
ownership. Archived posts cannot use either proof. Migration 0139 retires their
remaining legacy deletion hashes; historical ownership is not reconstructed.
No plaintext password, raw address or proof digest is added to audit records.

Password verification happens before taking content locks. The mutation then
locks board, thread and post, rechecks the current peer history and compares the
current encoded password hash with the verified fingerprint. A password rotation
while waiting therefore invalidates the earlier proof. File marking, report
handling and audit insertion commit together.

## Database boundary and rollout

Two narrow helpers expose an active public post's encoded recovery hash and a
boolean peer match to the staff role. They use existing NOLOGIN owners and table
privileges. The runtime receives no bulk access to the private tables. Hash
lookup includes replies, extending the existing OP-only staff lookup. Peer
matching also supports private-board history, subject to ordinary staff board
authorization in the handler.

Readiness checks validate helper signatures, bodies, owners, execution grants,
security mode and fixed search paths. Deletion repeats this check inside its
transaction. The migration checks and reseals the thread-transfer catalog and
extends the audit action constraint. Shared report readiness recognizes the same
expanded constraint.

Apply the forward migration while writers are stopped, then deploy matching
binaries. Older binaries fail the expanded audit-constraint readiness check.
This migration does not rewrite historical posts, reports or audit records and
does not backfill past self-deletion events. Reverting application code cannot
restore reports already removed by earlier administrative requests.

## Qualification and related behavior

`apps/staff/tests/self_deletion.rs` exercises the real staff HTTP route with
owned disposable PostgreSQL fixtures. It covers peer and password proof,
administrative fallback, incorrect and rotated passwords, unchanged authorization,
whole deletion, runtime role denials, catalog drift and audit rollback. The staff
browser flow exercises the optional password with JavaScript disabled, checks
that the report remains, then verifies ordinary administrative cleanup.
`scripts/test-staff-self-deletion-migration.sh` checks fresh installation, populated
upgrade, unchanged historical data, role denials, archive retirement, restore and
rejection of preexisting catalog drift. Exact executed results are recorded in
the pull request.

Historical cleanup and media reconciliation remain explicit operator procedures.
Independent audit content uses the [30-day retention policy](moderation-snapshot-retention.md).
[Deletion parity](deletion-parity.md) describes the full issue coverage and security
differences. None of these operations claims secure erasure from backups. See [fresh content erasure](fresh-content-erasure.md),
[file-only actions](file-deletion-actions.md) and the
[archive lifecycle audit](archive-lifecycle-audit.md).
