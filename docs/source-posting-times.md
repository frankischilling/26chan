# Posting and change clocks

[Issue #124](https://github.com/frankischilling/26chan/issues/124) implements
the supplied posting timestamp assignments. `imgboard.php:4900` captures
`REQUEST_TIME`; lines 5362-5364 derive the displayed date from it, and
6524-6544 insert that integer into `time` and `last_modified`. Replies update
the OP's `last_modified` from `REQUEST_TIME` at 6668. The root ordering clock
is separate: new threads use `now()` at 6145, and eligible replies use
`root=now()` at 6648/6657.

The public middleware captures server time before collecting the form,
hashing the deletion password or waiting for database connections and locks.
Both posting aliases and encodings carry this context into the transaction.
Post creation time and the thread's public modification time use its whole
Unix second; a new thread's creation time matches its OP. Internal store
callers capture their entry time unless they explicitly supply a server-owned
context. Form fields, headers and pre-existing extensions cannot set it.
The bump clock remains database time. Source age and OP self-bump comparisons
therefore read the same persisted post seconds that the public API displays.

An older request can commit after a newer one. The source modification time
then regresses, while post numbers retain serialized insertion order. Several
posts can also share one second. `threads.json` and `catalog.json` expose the
source modification time, and each post's JSON/HTML date uses its saved time.
Do not interpret those values as unique IDs or commit-order markers.

## HTTP validators

`content.threads.http_modified_at` is a separate database change clock for
thread JSON, tail JSON and native updater responses. An invoker trigger
advances it on thread updates, including moderation, without allowing the
public runtime to assign the column. It never moves behind its previous
value. This keeps an old request's source timestamp from making changed
content appear older to an `If-Modified-Since` client. The field is not added
to JSON bodies. Body-derived ETags remain authoritative, and date comparisons
still revalidate same-second changes. Board lists use body-derived ETags.

The source also separates post metadata from regenerated-file modification
time; this database clock is the rewrite's cache implementation, not a new
source field. It is not a wall-clock synchronization guarantee or a commit
sequence number. Operators must synchronize application and database hosts.

## Migration and authority

Apply migration 0025 after 0024 and before deploying this binary. It preserves
historical post, thread creation, modification and bump timestamps. Existing
threads receive a new HTTP change clock during migration, causing cache
revalidation. The security-barrier visible-thread view retains its visibility
predicate and grants.

Public posting receives creation-time INSERT grants only. It cannot update
historical creation times or explicitly insert/update the HTTP clock.
The attachment-owner role receives only the post creation-time INSERT grant.
The new ten-argument attachment function rejects null/infinite timestamps
and truncates valid times to seconds. Its fixed search path, ownership,
restricted execution, board/thread/job lock order, approval, image count and
single-use checks remain in force. Capability expiry uses the actual database
clock after lock acquisition; an old posting request cannot extend it.

The nine-argument attachment entry point remains available for older binaries
and uses current database seconds as its fallback. Keep the additive schema
on binary rollback; an older binary resumes its database posting-clock
behavior. Neither this migration nor rollback reconstructs unknown historical
request arrival times. Public database compromise still permits fabricated
new public content and times, but adds no staff, deployment or media decoder
authority.

## Verification

`apps/public/tests/posting_times.rs` covers exact whole-second persistence,
same-second ETag changes, matching ETag 304s, older-request modification-time
regression, date revalidation, unchanged sage bump clocks, JSON metadata and
delayed streamed bodies through both aliases and encodings. The existing
age lock-wait test also checks the saved request timestamp. The OP concurrency
test witnesses actual root changes with an owned test-only audit trigger,
since request time cannot identify which transaction changed the root.

`crates/store/tests/post_media.rs` covers attachment and attachment-only
timestamps, historical UPDATE denials, invalid timestamp rejection,
nine-argument compatibility and expired capabilities with old request times.
`scripts/test-posting-time-migration.sh` upgrades owned historical rows and
tests real public/staff identities, insert-only grants, view/function safety,
independent clocks and rollback. The restore exercise compares full thread
fingerprints, including both clocks, alongside its existing post/private/media
checks.

Local domain/public library tests and all-target/all-feature Clippy pass.
PostgreSQL is unavailable locally; the persisted, upgrade and restored-clock
tests require complete current-head Linux CI before merge. No visual baseline
or decoder boundary changes. Deletion/moderation source timestamp details,
other cooldowns, complete posting admission and deployed production
qualification remain separate unfinished work.
