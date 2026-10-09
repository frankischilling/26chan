# Content after whole-post deletion

Migration 0123 removes post payload and per-post author authority when a live post
or thread is deleted. The supplied `4chan-old/imgboard.php:2438–2770` physically
removes post rows. The rewrite keeps a small structural tombstone because its
media security model depends on durable attachment associations.

## What changes

Whole-reply deletion clears the post's name, subject, comment, formatted payload,
tripcode, poster IDs, country and board flags, dice and fortune values, timestamps
and presentation metadata. It also removes deletion passwords, anonymous ownership,
OP ownership links, poster context, posting history and staff discussion authorship
for that post. A whole-thread deletion applies this to every descendant, including
replies that were already soft-deleted, and clears the thread's metadata.

An irreversible marker prevents restoring the cleared payload, undeleting the
row or attaching new author authority to it. Post IDs and thread IDs/boards
cannot be reassigned, including by the migration role. Guarded live-post moves
revalidate author-link targets after any lock wait. A live thread's remaining posts and
normal update behavior are preserved. Thread expiry and rollover use the same
whole-deletion path. Archiving without deletion and file-only deletion retain their
existing behavior.

The migration itself does not sweep previously deleted content. Historical cleanup
still needs a bounded operator procedure and an explicit retention decision.

## Transaction and permissions

The erasure runs in the deleting transaction. A failed authorization, audit write
or later operation rolls it back. Board locks serialize new posts and author links
with deletion. Deletion requires Read Committed isolation; tuple-first operations
use nonwaiting lock acquisition to avoid reversing the normal lock order.

A separate NOLOGIN owner can read structural identifiers, update the descendant
`deleted` flag and delete the matching author links. It cannot read comments,
password hashes or ownership proofs. The only additional application-function
permission is the immutable board-flag label lookup required by an existing CHECK
constraint. Runtime roles cannot call the private trigger functions directly.
Public and staff readiness checks verify role, policy, privilege and trigger setup.

## Retained records and limits

The post and thread identity rows and `post_media` associations remain. Removing
those associations would make an already consumed upload look unattached and could
reopen an approved media URL. The retained ledger keeps deleted attachments hidden
and prevents reusing their upload capabilities, including after database restore.
Media cleanup remains a separate operation.

Independent moderation audit records, shared sessions, cooldown and quota state,
and Robot9000 state are unchanged. This work creates no new copy of deleted post
content. It does not settle retention of pre-existing audit data or media metadata.

This is logical removal from the current database schema. It is not secure disk
erasure and does not erase WAL, backups, logs or copies already held by clients.
It narrows issue #214 but does not establish complete retention or hard-delete
parity.

## Upgrade and verification

Stop writers, bootstrap the new restricted owner, then apply the migration before
starting matching public and staff binaries. An older binary cannot recover erased
content. Recovery of an earlier database state requires the established backup
and restoration procedure.

Tests cover actual runtime roles, rollback, concurrent author-link writes,
irreversible tombstones, archive versus deletion behavior, public endpoint removal
and media capability consumption. The migration qualification script checks a
populated upgrade, policy and privilege drift, logical content removal and an
administrator dump/restore. Execution results belong in the PR; this list is a
description of the tests, not a claim that every environment has passed.
