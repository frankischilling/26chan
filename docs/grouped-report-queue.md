# Grouped staff report inventory

The staff queue groups uncleared retained reports by board and post. Each group
has one selected post preview. Resolved and dismissed reports stay in the group
until a separate clear or deletion action removes them. Both reporter-clear and
group-clear markers exclude rows from this inventory.

This follows the grouping and filter shape in
`reports/ReportQueue.php:771–825,2290–2473` and the board query in
`reports/js/031d60ebf8d41a9f/reportqueue.js:2355–2392`. It is a partial #219 slice.
The queue does not calculate source weights, ignored membership, highlights,
weighted board counts or cross-board unlock. The
[weight precision boundary](report-weight-evidence.md#source-numeric-precision-boundary)
remains unresolved.

## Selection and bounds

Groups with an open report come first, followed by latest report ID descending
and board/post tie-breakers. The selected report is the newest open member, or
the newest member when none is open. This is a deterministic unweighted fallback.
The source grouped SQL selects a representative ID, timestamp and saved JSON
without specifying which member supplies them; this ordering is not presented
as source weighted priority.

The existing preview budget is now 100 groups instead of 100 report rows.
Grouping happens before that limit. A 101st lightweight group detects overflow
and produces a notice asking the operator to narrow the filter. The source caps
3000 qualifying groups before weighted sorting. This slice retains the smaller
preview bound and has no pagination. Moderator category summaries show at most
16 distinct captured entries, with an explicit notice if more exist.

Category summaries scan the selected retained inventory once, deduplicate captured
entries, and rank them per group inside PostgreSQL. The query returns at most 17
entries per group, including its sentinel; Janitors do not execute it.

Selection, report projections, categories and media use one read-only Repeatable
Read snapshot. The handler revalidates current staff authority before returning
the response. A retained report can outlive private active membership, so its
presence does not prove that a group-clear request will succeed.

## Filters and presentation

The optional `board` query accepts a board slug, `_ws_`, `_nws_`, or an empty
value for all authorized boards. A named board must pass account scope before
its existence is checked. Explicit denials override `all`. WS/NWS filters use
current board configuration; they do not reconstruct the source's report-time
WS capture. Duplicate parameters, unknown switches, malformed encodings and
oversized queries are rejected.

Moderator-or-higher summaries show the number of uncleared retained reports and
captured category revision, ID, kind and title. These are neither active-member
counts nor numeric weight proofs. Different captured titles or revisions remain
distinct even when their category IDs match.

Janitor queue responses omit group member counts, categories, category titles,
selected-report reasons, weights and highlights. The server does not emit hidden
member lists or metadata that would reveal those values. The selected report ID
and disposition remain available for existing individual actions. The separate
cleared-history page also omits category, reason and originating clear account
for Janitors, with redaction in both SQL and rendering. It retains each report's
disposition, clear time and inherited-clear flag. Moderators retain the captured
category, reason and clear account. This does not claim comprehensive redaction
across unrelated audit surfaces.

New reports retain public post presentation at admission. Previews label the
capture time and use saved text, identity presentation and attachment metadata.
Current media availability is checked separately, so removal or expiry cannot
reopen a saved attachment. Older reports without a capture are explicitly
labeled as current-data previews. Deleted posts and threads are excluded from
this queue; retained archives keep the existing visibility rules. See
[report captures](report-post-snapshots.md) for storage and migration limits.

## Actions

Resolve and dismiss change only the selected report's disposition. They do not
clear its group. Refreshing can select a different open report from that group.
Group clear submits the existing board/post target. Reporter clear uses the
selected report's reporter under the existing global permission; it does not
mean every reporter in the group.

The existing origin, CSRF, recent-authentication, live authority, transaction and
audit checks still apply. No bulk resolve/dismiss operation or new private-table
grant is added. Group clearing retains its unknown-weight, missing-membership,
already-cleared and capacity failures. The inventory never invents eligibility
from a displayed count or category base.

## Remaining work

Full source priority, ignored membership, exact weighted cleared-only membership,
full source snapshot fields, source weighted counts, the larger source cap and
global unlock remain separate. [Cleared history](grouped-cleared-report-history.md)
groups retained clear evidence with its own bounded, unweighted semantics.
The source's developer-only explicit `test` filter also needs separate policy
work; this slice does not silently change access to local fixture boards.

## Verification

The local staff suite passed 219 tests, including grouped reads, captured category
bounds, role-safe queue/history responses, action isolation, authority expiry and
a concurrent-write snapshot check. The snapshot fixture verifies metadata
visibility, not image decoding or publication.

Two Unix-socket tests cannot run under this environment's socket restrictions.
The explicitly ignored browser-backed upload test remains for hosted CI. The
local staff browser suite could not launch because the pinned Chromium download
returned an invalid archive; no browser pass is claimed here. Hosted CI must run
those checks before merge.

A synthetic plan check with 50,100 uncategorized reports and 100 selected groups
confirmed one report-table scan after the category-query change, replacing 100
scans. Local execution fell from about 370 ms to 9 ms. This identifies avoidable
work; it does not establish the exact cause of the earlier hosted 503 because
that run did not retain a database error code. Database-test builds now report
only a sanitized SQLSTATE or an internal-failure label, never SQL or bind values.
