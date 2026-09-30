# Private poster counts

Active threads expose the optional `unique_ips` integer when every visible post
has a verified network context from one key epoch. The count includes the opening
post. Full and tail thread JSON, index JSON and catalog JSON place it on the
opening post only. Native statistics use the same count and show `Posters`.

The [pinned Threads API](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/pages/Threads.md)
defines the field. Public extension v1191 supplies its statistics presentation;
the release is pinned in [the asset manifest](public-watcher-assets.json).
These references do not establish the original server's private counting
implementation. This keyed implementation is the E-004 privacy replacement.

## Counting policy

The posting transaction uses `POSTER_ID_KEY` and the verified connection peer
used for [poster IDs](poster-ids.md). Counts also work with public labels disabled.
IPv4-mapped IPv6 addresses use canonical IPv4. Ordinary forwarding headers and
form values cannot select the peer.

Each new post receives a full HMAC-SHA256 fingerprint scoped to its board and
thread, plus a keyed epoch marker. It uses a separate version prefix from public
eight-character IDs. Truncated labels and rendered posts do not supply the count.
A shared address counts once; this measures network contexts, not people or
accounts. Address changes can count twice.

A thread qualifies only with 1 through 1001 visible posts, a context for every
visible post and exactly one epoch. Missing keys or peers, historical posts and
mixed-key threads leave the field absent. They do not produce zero or partial
counts. Deleting the last unqualified visible post can restore a complete count.
Rotation changes new fingerprints and can make existing threads unqualified.
Archived threads always omit the count.

HTTP writes without trusted connection identity fail with 503, including those
that supply ordinary forwarding headers. Internal or historical writes without
capture remain unqualified. The counting policy preserves that transport rule.

## Storage and authority

Migration 0040 creates `post_secrets.poster_contexts`, storing post/thread IDs,
32-byte fingerprints and 32-byte epoch markers with no new raw addresses.
Triggers remove contexts on post/thread deletion and thread archival; foreign
keys remove them on physical deletion. Historical content and clocks stay intact.

Public, staff, authentication, media coordinator, approved-media reader, intake
and monitoring logins cannot read or mutate this table. A separate
`board_poster_count_owner` NOLOGIN role owns fixed-search-path functions with
only the required column reads and private-table SELECT, INSERT and DELETE.
It has no staff/deployment access or schema creation privilege. Public SQL can
execute only the aggregate function, cannot call trigger functions and cannot
assume their owner role. Administrators and the migration role retain explicit
maintenance authority.

Transaction-local values carry the fingerprint and epoch into the insert
trigger, including scoped attachment insertion. Missing both values records no
context; malformed or incomplete values reject the write. A compromised public
runtime can falsify contexts supplied for its own writes. The count grants no
authentication, moderation or deployment authority.

Thread and board JSON read the aggregate inside their existing repeatable-read
snapshot. Native statistics use one database statement. No private fingerprint
crosses the public database boundary, JSON, DOM or logs.

## Upgrade and rollback

Fresh installations use the updated `deploy/roles.sql`. Before migration 0040
on an existing installation, a bootstrap administrator runs
`deploy/poster-count-role.sql` once; the migration owner then runs `board-migrate`.
Neither public nor staff credentials bootstrap roles.

Configure the private `POSTER_ID_KEY` as described in the poster-ID notes.
Keep it stable across public instances sharing a database and back it up
privately for continuity. Restoring with a different key leaves mixed-epoch
threads unqualified. Counts do not require public labels. No key is generated
automatically.

The additive migration can remain during application rollback. Older binaries
do not capture new contexts, so threads receiving their writes omit the count.
Deletion and archive cleanup still run. Rollback or key destruction cannot
reconstruct omitted historical identities.

## Verification

`cargo test -p board-domain --lib poster_id --locked` checks independent Python
HMAC vectors, scope changes, mapped addresses and epochs.
`cargo test -p board-public --test poster_counts --all-features --locked`
checks persisted full/tail/index/catalog fields, deletion, missing capture,
history, rotation, archival cleanup and denials under seven actual runtime
logins. One public pool connection checks transaction-local isolation. The
router fixture supplies trusted synthetic connection metadata; actual
socket/proxy enforcement has separate transport tests.

`sudo bash scripts/test-role-bootstrap.sh` applies all migrations to a private
disposable cluster and compares pre-0040 posts and thread clocks in both
directions. It checks function-owner and runtime grants.
`npm run test:stats-core` covers the optional field, strict bounds, archive
omission and removal of a previously displayed count. Persisted statistics
browser tests exercise the endpoint, desktop/mobile placement and preferences.
Executed outcomes belong in [the verification record](verification-rewrite-completion.md).
