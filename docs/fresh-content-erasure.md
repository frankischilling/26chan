# Content after whole-post deletion

Migration 0129 removes post payload and per-post author authority when a live post
or thread is deleted. The supplied `4chan-old/imgboard.php:2438–2770` physically
removes post rows. The rewrite keeps a small structural tombstone because its
media security model depends on durable attachment associations.

## What changes

Whole-reply deletion clears the post's name, subject, comment, formatted payload,
tripcode, poster IDs, country and board flags, dice and fortune values, drawing
time and source-post annotations, timestamps and presentation metadata. It also
removes deletion passwords, anonymous ownership, OP ownership links, poster context,
posting history and staff discussion authorship for that post. A whole-thread deletion applies this to every descendant, including
replies that were already soft-deleted, and clears the thread's metadata.

An irreversible marker prevents restoring the cleared payload, undeleting the
row or attaching new author authority to it. Post IDs and thread IDs/boards
cannot be reassigned, including by the migration role. Guarded live-post moves
revalidate author-link targets after any lock wait. A live thread's remaining posts
and normal update behavior are preserved. Thread expiry, rollover and sticky reply
pruning use the same whole-deletion path. Archiving without deletion and file-only
deletion retain their existing behavior.

The migration itself does not sweep previously deleted content. Migration 0141
adds an [explicit bounded operator procedure](historical-deletion-cleanup.md)
with a preview and reviewed receipt; it is never run automatically by an upgrade.

## Transaction and permissions

The erasure runs in the deleting transaction. A failed authorization, audit write
or later operation rolls it back. Board locks serialize new posts and author links
with deletion. Deletion requires Read Committed isolation; tuple-first operations
use nonwaiting lock acquisition to avoid reversing the normal lock order.

A separate NOLOGIN owner can read structural identifiers, update the descendant
`deleted` flag and delete the matching author links. It cannot read comments,
password hashes or ownership proofs. It can call the immutable board-flag label
lookup required by an existing CHECK constraint. Runtime roles cannot call the private trigger functions directly.
Public and staff readiness checks verify role, policy, privilege and trigger setup.
A shared catalog-only visibility check runs once before each INSERT or UPDATE
statement on posts, threads and author-link tables, including statements that
find no visible rows. The deferred staff association guard checks it again before
its final row check, and whole-thread deletion rechecks after its descendant
UPDATE. The per-row identity and board-lock checks are unchanged. It rejects changes to the required row-security policies, applicable
restrictive policies and unexpected row security on the other author-link tables.
Detected drift makes the operation fail and roll back before cleanup can silently
skip content or authority. The readiness helper runs with caller permissions and
exposes only a boolean; public and staff roles gain no private-schema or table
access. Readiness treats a false result or database error as unavailable. Revoking
the caller's helper EXECUTE permission can make PostgreSQL reject the query before
it evaluates the CASE condition. Schema and policy changes require stopped
writers; this preflight does not coordinate concurrent operator DDL.

## Retained records and limits

The post and thread identity rows and `post_media` associations remain. Removing
those associations would make an already consumed upload look unattached and could
reopen an approved media URL. The retained ledger keeps deleted attachments hidden
and prevents reusing their upload capabilities, including after database restore.
Media cleanup remains a separate operation.

Independent moderation audit records, including saved drawing annotations, are
unchanged. Shared sessions, cooldown and quota state, and Robot9000 state are also
retained. Poll configuration, options and ballot receipts belong to global polls
with no post or thread association, so whole-post deletion leaves them intact.
This work creates no new copy of deleted post content. It does not settle retention
of pre-existing audit data or media metadata.

This is logical removal from the current database schema. It is not secure disk
erasure and does not erase WAL, backups, logs or copies already held by clients.
It narrows issue #214 but does not establish complete retention or hard-delete
parity.

## Upgrade and verification

Stop writers, bootstrap the new restricted owner with
`deploy/content-erasure-role.sql`, then apply migration 0129 after the current
0128 schema before starting matching public and staff binaries. Fresh databases
use `deploy/roles.sql`, which includes both the poll and erasure owners. An older
binary cannot recover erased content. Recovery of an earlier database state
requires the established backup and restoration procedure.

Tests cover actual runtime roles, rollback, concurrent author-link writes,
irreversible tombstones, archive versus deletion behavior, public endpoint removal
and media capability consumption. The migration qualification script checks a
populated upgrade, policy and privilege drift, logical content removal and an
administrator dump/restore. Execution results belong in the PR; this list is a
description of the tests, not a claim that every environment has passed.
