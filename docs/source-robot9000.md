# Robot9000 posting rules

Public posting now applies the active `plugins/robot9000.php` rules from the
supplied `4chan-old` revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
Migration 0062 enables the robot on `/r9k/`; the other supplied board
configurations leave it disabled. The inventory test compares that switch with
the pinned board fixture.

The robot rejects empty and non-ASCII comments. It reduces the prepared
comment's source HTML by removing generated tags, quote references and entities,
lowercasing, retaining only ASCII letters, digits and hyphens, removing leading
digits and collapsing runs of three or more identical characters. The source's
greedy numeric expression retains trailing digits; the rewrite retains that
result. For source HTML longer than ten bytes, normalized text below ten percent
of the stripped comment length causes a low-content mute.

An accepted normalized comment enters board-specific history. Repeating it
refreshes its history timestamp and mutes the poster. The first violation lasts
two seconds, successive violations double the duration, and the duration caps
at one year with stored power 24. An active mute rejects new text without
extending itself or adding history. Empty/non-ASCII checks retain their earlier
position in the plugin's check order. A successful post after expiry decays one
level and postpones the next decay by one day. Elapsed days do not cause several
levels of automatic decay. Deleting a post retains its originality history.

The source's image duplicate branch and old secret moderator commands are
commented out. They do not form part of this active plugin. Authenticated
capcoded staff posts bypass it without registering history. Public options,
headers and cookies cannot confer that authority. Anonymous staff posting with
the explicit janitor bypass is implemented in the authenticated staff form; see
[staff posting authority](staff-posting-authority.md). The
[staff cleanup interface](staff-robot9000-cleanup.md) applies the source two-year
text-history retention in bounded, audited batches.

Rejections use the existing escaped HTML error page, or the source JSON posting
envelope containing only `error`. Quick Reply keeps the failed draft and
password. Infrastructure and capacity failures retain HTTP 503 rather than
being converted into a successful post. The [verification record](verification-robot9000.md)
lists the actual checks and failed attempts.

## Private state and transaction boundary

History and mutes live in `post_secrets`, outside every runtime's direct table
grants. A NOLOGIN `board_robot9000_owner` owns a fixed-search-path function and
has only the board columns needed to lock/check policy and SELECT/INSERT/UPDATE
on these two tables. Migration 0117 adds text-history DELETE and column-scoped
cleanup-audit INSERT for the staff cleanup function. It cannot read posts, deletion passwords, staff
credentials, media or deployment settings. `board_public` and `board_staff` can execute the
bounded public-board check; runtime roles cannot assume its owner identity or
change its policy. The ordinary public runtime retains its existing content
authority, including the ability to see submitted comments.
An already-compromised public process could bypass application posting rules
or fill the bounded robot state through its check function. This boundary
protects private tables and unrelated staff/media authority; it does not make
the public process's own posting decisions trustworthy after compromise.

The store locks the board before a savepoint. It performs the normal thread,
attachment and posting checks, then checks originality before commit. A robot
rejection rolls back rollover, posts, counters, clocks, attachments and deletion
secrets. The retained board lock permits the same decision to be applied again
and committed with only mute/history state. Acceptance commits its post and
new history together. Errors roll back both. Post-number sequence gaps remain
possible after a failed transaction, as with other posting failures.

Each board has a validated state limit, initially 100,000 text hashes and
100,000 mute records. New state at capacity fails closed. Board locking
serializes duplicate decisions; database statement and lock deadlines still
apply. Comment projection accepts at most 131,072 input bytes and a zero-length
stripped denominator is treated as zero content. These bounds replace unsafe
arithmetic and unbounded accumulation in the source.

Text history uses SHA-256 rather than MD5. Actor keys use a board-specific,
domain-separated HMAC of the verified canonical socket address and
`POSTER_ID_KEY`, replacing the stored integer IP. IPv4-mapped IPv6 addresses
identify the same actor; arbitrary forwarded headers do not. Actor keys, hashes,
power and expiry fields do not enter public JSON, pages or metrics. This key is
required for robot-enabled public posting. Rotating it starts a new actor
namespace while leaving text history intact; plan rotation as an explicit
anti-abuse state change. Mute timestamps use America/New_York, consistent with
the existing public date rendering. The supplied plugin relies on the PHP
process timezone without specifying it locally; this is a documented assumption.

## Upgrade, retention and rollback

Fresh installations receive the owner from `deploy/roles.sql`. On an existing
installation, the bootstrap administrator must create it and grant only the
migrator permission to assume it before running migration 0062:

```sql
CREATE ROLE board_robot9000_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOREPLICATION NOBYPASSRLS;
GRANT board_robot9000_owner TO board_migrator WITH INHERIT FALSE, SET TRUE;
```

Run the normal migrator outside application processes, then deploy the binary.
Retain the added columns and private tables for binary rollback. An earlier
binary will not apply the new robot rule, so reverting it changes posting policy.
Historical posts are neither rewritten nor treated as evidence of originality.

The source cleanup removes text history older than two years; it does not prune
mute records. Staff can use the authenticated cleanup interface. An operator with migration
credentials can also remove a bounded batch under the same board lock. Repeat the transaction as needed and observe the
configured database deadlines:

```sql
BEGIN;
SELECT slug FROM content.boards WHERE slug='r9k' FOR UPDATE;
DELETE FROM post_secrets.robot9000_texts
WHERE board='r9k' AND digest IN (
    SELECT digest FROM post_secrets.robot9000_texts
    WHERE board='r9k' AND seen_at<((clock_timestamp() AT TIME ZONE 'UTC')-interval '2 years') AT TIME ZONE 'UTC'
    ORDER BY seen_at,digest LIMIT 1000
);
COMMIT;
```

Back up these tables with the rest of the database. Staff/public credentials
have no direct deletion grant. Restore and current-head CI qualification remain
separate release requirements, and the complete rewrite remains open in #191.
