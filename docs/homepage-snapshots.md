# Homepage snapshots

Migration 0156 adds a bounded cache of public post identities and local statistics.
Refresh it with the migration account:

```sh
cargo run -p board-store --bin board-homepage -- refresh
```

The command reads `MIGRATION_DATABASE_URL` and requires both the session and
current role to be `board_migrator`. It does not install a scheduler. An operator
can run it every five minutes; the page marks a snapshot older than 15 minutes
as stale. A failed refresh leaves the previous generation intact. Concurrent
refreshes can fail with a serialization error and should be retried on the next
scheduled run. Lock waits are limited to five seconds and each statement to 30
seconds.

## Posts shown on the homepage

Each generation stores at most 12 identities for each of Popular Threads, Latest
Posts and Latest Images. No copied post text is stored. Public reads join those
identities back to current posts, boards, reports and approved media. Deleted,
archived, private, reported and spoiler content is excluded at read time, even
before another refresh. Removing a file removes its image card. Excerpts are
limited to 300 characters and rendered as escaped text.

The selection follows the supplied `update_fpcontent.php` producer: posts must
be older than ten minutes; `/b/`, `/f/` and `/qa/` are excluded; literal source
spoiler and filename checks apply. Latest-post and image selection takes five
candidates per board before filtering them. Popular threads need 15 actual
reply rows and a recent bump or sticky status. The source encodes sticky roots
as future timestamps. The typed sticky flag represents that condition here.
The independent PHP fixture covers the captured source filter.

## What the numbers mean

These are local measurements for the first 100 public boards:

- Recorded posts counts retained post rows, including rows marked deleted. It is
  not a historical lifetime counter and can fall after physical retention cleanup.
- Active content counts approved attachment bytes on live, unarchived posts.
- Recent posting sessions counts unexpired anonymous sessions active within 15
  minutes that own a live public post. It does not count readers or unique people.

The old producer summed per-board maximum IDs, file sizes and distinct stored
hosts. Imported historical totals and raw visitor addresses are not used. This
privacy change means the session count is not equivalent to the old visitor
count. If the public board roster changes, the old aggregate is hidden until
refresh. Per-board history retains 64 samples per board, with the same 100-board
bound; private boards disappear from its public projection immediately. `/stats`
shows the latest sample per public board; `/stats?board=g` shows that board's
bounded history. Missing or private board histories return 404.

## Qualification

Focused tests use an owned PostgreSQL database and the actual store module and
operator CLI. They cover source candidate ordering, sticky and ordinary bump
eligibility, actual reply counts, file deletion, reports, private boards,
staleness, generation exhaustion, retained snapshots and the 64-sample bound.
Concurrent refreshes retain complete generations. Concurrent editors using the
same page revision admit one update and reject the other; errors finish rollback
before returning the connection, so a rejected editor does not retain locks.
Nine schema drift controls cover visibility, grants, bounds and literal marker
whitespace. Readiness compares complete canonical view definitions without
stripping characters from quoted filter values. The actual
Askama homepage and posting-notice templates also compile in a focused harness.
The populated 0154 upgrade preserves boards, threads and posts; a schema-name
conflict rolls back, and an administrator dump/restore preserves public
projections and homepage readiness.

Full application HTTP tests and hosted browser qualification remain required
before the issue can close. The original homepage controller and template are
missing from the supplied source, so the layout is a reconstruction rather than
an assertion of historical pixel identity. See [the source inventory](homepage-source-parity.md).
