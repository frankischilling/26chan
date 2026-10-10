# Grouped cleared report history

The cleared-history page groups retained reports by board and post. It requires
one authorized board through `/reports/cleared?board=...`. Each group contains
reports with an originating group-clear marker and no reporter-clear marker.
Report disposition is independent of clearing, so open, resolved and dismissed
records can appear together.

The source groups cleared reports in `reports/ReportQueue.php:790–820` and
requires a named board for its cleared-only view at lines 2318–2327. Its view also
applies numeric weight rules. This page is an unweighted retained inventory;
exact source cleared-only membership still depends on the unresolved
[weight contract](report-weight-evidence.md#source-numeric-precision-boundary).

## Selection and provenance

The newest eligible report ID represents a group. Groups are ordered by that ID
descending. Grouping happens before the 100-group limit; a lightweight 101st group
detects overflow. This order and bound are explicit choices, not a reconstruction
of the source's unspecified representative selection or 3000-group cap.

The selected report keeps its own disposition, originating clear time and
inherited-clear flag. These fields describe that report, not every member of the
group. Moderators and higher roles can inspect up to 16 retained records per
group, each with its own provenance. Member overflow is stated explicitly.
Captured categories are summarized separately, with at most 16 distinct entries
and an overflow notice. Category revision, ID, kind and captured title identify
an entry; the page does not relabel old reports from the current catalog.

Janitors receive only the selected report's allowed fields. They receive no
member count, additional member list, member-overflow flag, captured categories,
report reasons or originating clear account. SQL projections enforce these
limits before rendering. The page-level overflow notice concerns groups only.

## Retention and access

Selection and bounded projections share one read-only Repeatable Read snapshot.
The handler checks live staff authority again before returning the page. Board
scope is checked before any data is loaded. As in the earlier history page, an
authorized, valid board slug with no retained reports returns an empty inventory,
even if that board does not exist. A denied board returns 403.

History does not require a live post or thread. It retains the records that still
exist after other actions; it cannot reconstruct evidence removed by canonical
whole-post erasure or administrative file deletion. No current post preview is
presented as a report-time snapshot.

This page has no resolve, dismiss, group-clear or reporter-clear controls. The
source client still renders ordinary Delete, File and Clear controls in cleared
mode (`reports/js/031d60ebf8d41a9f/reportqueue.js:2295–2309`); those actions remain
separate work. This page does not calculate weights, grant cross-board access,
restore private membership or change clear markers. Existing data retention and
database permissions apply.

## Verification

The local full staff suite passed 227 tests, including eight new grouped-history
integration cases and the updated inline rendering controls. Formatting and
strict all-target/all-feature staff Clippy passed. An independent static review
found no remaining access, bounding or snapshot defects.

Two Unix-socket tests remain blocked by the local environment, and one explicitly
ignored browser-backed upload test requires pinned Chromium. No local browser
pass is claimed; those checks remain mandatory in hosted CI before merge.
