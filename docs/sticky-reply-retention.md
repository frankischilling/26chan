# Sticky reply retention

A thread marked both sticky and undead keeps a rolling window of replies. On
the imported source boards, the window contains 1,000 visible replies plus the
opening post. A successful new reply keeps the newest 999 existing reply IDs,
retires every older reply, and then inserts the new post. Gaps from earlier
deletions do not occupy the window. Reply creation time and bump order do not
choose the victims.

This follows `imgboard.php:6484-6508` and the global `STICKY_CAP=1000` setting.
The rewrite uses the existing operator-owned `reply_limit` for this capacity;
every imported source board sets it to 1,000. Smaller synthetic capacities
exercise the same boundary in tests. The source pruning branch requires a cap
greater than one. A synthetic cap of one keeps the existing admission limit.

Sticky alone and undead alone retain ordinary reply admission. Closed and
archived thread rules still apply, including the existing staff permissions.
Upload preflight and the final posting transaction use the same exception for
an active sticky and undead thread. The final decision follows the board and
thread locks, so a queued post sees flag changes that committed while it waited.

## Posting and cleanup

Retirement runs after posting admission, cooldowns, and staff authorization,
inside the existing Robot9000 savepoint. The transaction holds the board and
thread locks through retirement, insertion, and commit. A failed post, rejected
duplicate, or unusable attachment rolls back both the new post and retirement
of the old replies. Concurrent accepted replies rotate the window in order.

The cached reply count for a qualifying thread becomes the surviving count
plus the incoming reply. Public JSON and HTML continue to count visible posts.
Ordinary threads retain their existing cached-count behavior.

Retired replies leave public thread, index, catalog, and tail snapshots. Existing
deletion triggers remove their reports and associated membership state, posting
history, and poster context. The media reader immediately rejects their files;
the existing media retirement process removes stored output later. This is a
visibility transition at commit, followed by physical media cleanup.

Migration 0125 also removes each newly pruned reply's deletion-password hash
and anonymous ownership proof. Its trigger uses the existing
`board_posting_cooldown_owner` role, which cannot log in or bypass row security.
It receives only post-ID selection and deletion on the anonymous membership
table. Application roles receive no private-table deletion privilege.

The transaction marker identifies the thread being pruned. The trigger checks
the actual invoking role, private-board access, current thread flags, and at
least `cap - 1` newer visible replies independently of that marker. A forged
marker cannot retire a surviving reply's proof. A caller holding a post lock
before the board lock fails immediately if another transaction owns the board,
preserving the existing lock order.

The migration applies to fresh deletion transitions. Historical rows keep their
existing retention policy. Retired post text remains in the soft-deleted base
rows; this change bounds the visible reply window, not total database storage.

## JSON and deployment

Full thread, tail, board index, and catalog JSON expose numeric `sticky_cap` on
the opening post when both flags are set. Replies omit it. The internal undead
flag stays out of JSON. Both JSON listeners use the same stored capacity, and
ETags change when the flags or visible replies change. Private boards, disabled
JSON, and deleted threads keep their existing visibility restrictions.

Apply the current migrations with writers stopped before starting the matching
binaries. Migration 0125 requires no new bootstrap role and performs no
historical deletion sweep. Both services' readiness checks inspect the trigger,
owner, event predicate, search path, and grants through PostgreSQL catalogs.
Missing or disabled retirement, broader credential access, or runtime deletion
grants make readiness fail.

## Qualification

`scripts/extract-sticky-retention-reference.py` checks the supplied source hashes
and evaluates its two selection predicates against independent SQLite fixtures.
It does not start the original PHP application. To compare the committed
fixture with the supplied checkout:

```bash
python3 scripts/extract-sticky-retention-reference.py --source /path/to/4chan-old
```

Database tests cover the exact 1,000-reply boundary, a preexisting excess,
deleted ID gaps, simultaneous writers, flag changes during a board-lock wait,
rollback, private-board authority, report and media visibility, and retirement
of hashes and proofs. HTTP tests compare full, tail, index, and catalog JSON on
both listeners and exercise upload preflight. Catalog-only readiness tests
introduce missing triggers and incorrect grants inside transactions that always
roll back.

`scripts/test-sticky-retention-migration.sh` upgrades a populated database from
0123 to 0125, checks that installation preserves existing content and private
state, and verifies that only eligible newly pruned replies lose credentials.
The required media and operations CI job runs this check in its own disposable
database, including direct runtime permission checks.

This covers the sticky reply window in issues #215 and #216. Complete archive
lifecycle, media metadata, and frontend parity remain tracked in those issues
and the [compatibility inventory](compatibility.md).
