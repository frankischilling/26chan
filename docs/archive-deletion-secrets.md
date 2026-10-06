# Archive deletion-password retirement

Migration 0091 retires deletion-password hashes when a thread first becomes
archived. The supplied `4chan-old/imgboard.php:1782-1800` clears `pwd` for the
OP and every reply in `archive_thread`. The rewrite deletes the corresponding
rows from `post_secrets.deletion` in the archive transaction. A failure rolls
back both retirement and the archive transition.

## Scope and retained data

Retirement runs only when `content.threads.archived_at` changes from null to a
timestamp. It covers the OP and all replies belonging to that thread, including
soft-deleted posts. Installing 0091 leaves hashes on already-archived threads
unchanged. The decision about a historical backfill remains pending; this
migration performs none.

Hashes on active threads remain, including Sticky and Undead threads and
active threads exempt from rollover on private boards. A soft-delete-only
rollover does not itself trigger archive retirement. Private-board archives
that do transition receive the same retirement as other archives.

This change does not erase post content, reports, audit records, media
tombstones or password-proof derivatives in `post_secrets.anonymous_posts`.
It does not change their existing lifecycle rules. It retires stored password
authority, without claiming complete privacy cleanup or physical erasure.

## Authority and concurrency

The trigger functions use the existing `board_posting_cooldown_owner`, a
NOLOGIN, NOBYPASSRLS role with a fixed `pg_catalog,pg_temp` search path. Its
additional deletion-table privileges are `SELECT(post_id)` and `DELETE`;
it receives no `SELECT(password_hash)`. The narrow thread `UPDATE(id)` grant
supports row locking. No new bootstrap role, runtime deletion-table `DELETE`
grant or RLS bypass is added.

A `BEFORE INSERT OR UPDATE OR DELETE` guard checks the actual caller's board
visibility. Insert and update reject secrets for archived threads and lock both
old and new parent threads for a reassignment. Runtime callers cannot restore
or rotate a hash on an archived post, or move a retained historical archived hash to an active
post. Undelete does not reconstruct a retired hash. Missing, hidden and
archived targets use the same generic rejection on insert or update. Deletion
allows archived and soft-deleted parents so retirement can revoke their hashes;
it reenters the board lock with `NOWAIT` without acquiring parent-thread locks.

Normal posting keeps its existing lock, proof and cooldown order. Operator
archive transitions must use Read Committed isolation. Repeatable Read and
Serializable transactions fail with SQLSTATE `22023`: a fixed snapshot could
otherwise miss a secret inserted before the archive obtained its locks.

Operator transactions must lock all affected board rows in slug order before
secret `UPDATE` or `DELETE` operations or archive writes. A raw out-of-order
secret update, secret deletion or archive can fail with SQLSTATE `55P03`
instead of waiting while holding a secret or thread row in reverse order.
Roll back and retry the transaction
with the board locks acquired first. These failures do not leave a partially
archived thread or partially retired password set.

## Deployment and recovery

Stop public and staff writers, apply migrations through 0091, and deploy
matching binaries before restoring posting. Existing bootstrap prerequisites
are in [posting deployment](source-posting-cooldowns.md#deployment). Migration
0091 reuses existing owners and requires no additional bootstrap role.

Both applications' readiness checks require the 0091 trigger/function
metadata and restricted grants. They inspect database catalogs without
reading hashes or invoking retirement. Readiness does not replace migration
qualification or establish production readiness.

Backups may contain hashes retired after the backup was taken. Restoring a
current snapshot preserves its exact current state, including retained legacy
hashes and the absence of newly retired hashes. It does not cleanse older
backups or retroactively apply retirement to already-archived rows. Backup
retention and deletion remain separate operator responsibilities; see
[backup and recovery](operations.md#backup-and-recovery).

This document records the implementation boundary. Current-head test results
and CI completion require separate qualification; no pass is claimed here.
