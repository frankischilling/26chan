# Local information pages and news

The homepage links to local information pages and a news archive. Operator content is escaped plain text. It cannot introduce scripts, images, embedded forms or external requests. The supplied historical site does not include its information-page controllers/templates, so this installation starts with local usage help and explicit empty states. It does not import another site's contact details, legal identity or payment instructions.

`/news` is the site news archive. `/news/` remains the original news board. News permalinks use `/news?item=ID#ID`; archive pages use `/news?before=ID`. A page displays at most 25 entries. The homepage shows the latest three published entries. Retraction removes an entry from both places and makes its permalink return 404.

## Operator commands

`board-site-content` uses `MIGRATION_DATABASE_URL` for maintenance. Both the actual login and current database role must be `board_migrator`. Public, staff and authentication roles have no maintenance API. The `validate` command is offline and does not read database configuration.

```text
board-site-content validate page.json
board-site-content page-revision faq
board-site-content publish page.json
board-site-content retract-page faq 1
board-site-content retract-news 1
```

A new page uses revision zero. Updating or retracting a page requires its current revision. A stale revision fails without changing content. Retraction retains the record and increments its revision; publishing against that new revision restores it.

```json
{
  "kind": "page",
  "version": 1,
  "key": "faq",
  "expected_revision": 0,
  "title": "Local FAQ",
  "body": "Operator-maintained help for this installation."
}
```

Page keys are `about`, `faq`, `rules`, `contact`, `feedback`, `legal`, `press`, `advertise`, `support`, `japanese`, `blog`, or `rules/BOARD`. A board-specific rules key requires an existing board. Its text is hidden whenever that board is private. The rules page supplies anchors for visible boards, including those without additional operator rules. The built-in FAQ is shown until an operator publishes an override, and returns if that override is retracted.

News entries have a title, optional author, plain-text body and Unix-seconds publication time. The following is an intentionally old development example; choose the actual publication time for a real announcement.

```json
{
  "kind": "news",
  "version": 1,
  "published_at": 1,
  "title": "Local development announcement",
  "author": "Site operator",
  "body": "A synthetic announcement for a disposable installation."
}
```

News timestamps must be newer than all retained entries, including retracted ones. Future entries stay hidden until their publication time. Dates are labeled UTC. Corrections append a new entry; retraction retains the old record. At most 10,000 news entries are retained. Maintenance stops at that budget instead of deleting history automatically.

## Bounds and publication checks

Input is capped at 32,768 bytes before JSON parsing. Titles are limited to 256 UTF-8 bytes, page bodies to 8,192, news bodies to 16,384, and optional authors to 128. Titles and authors cannot contain control characters. Bodies permit line feed and tab, but reject other controls and whitespace-only content. Unknown fields and invalid keys are rejected.

Migration 0155 keeps records in `site_private`. Security-barrier views expose only published pages, public-board rules and news whose publication time has arrived. The public role receives SELECT on those views. Readiness checks their owners, publication filters, barriers, grants and bounded-record constraints; broadened grants or a missing filter make readiness fail. Retractions and page updates serialize with publication, and page revisions prevent stale replacement.

## Qualification

The exact storage module and command parser pass focused offline tests. A fresh disposable PostgreSQL schema passes publication, revision fencing, retraction, future-news exclusion, pagination, private-board rules, role isolation and seven readiness-drift controls. The actual Askama information/news templates pass focused escaping and navigation checks with reduced fixture models.

These focused checks do not establish a full application pass; [PR #289](https://github.com/frankischilling/26chan/pull/289) records full HTTP and hosted results. Homepage statistics, promoted content and source posting notices are documented in [homepage snapshots](homepage-snapshots.md). Every final-head check must pass before merge.

## Global announcement

The `announcement` page key publishes the [global board announcement](global-announcement.md)
after migration 0157. It uses the same revision-fenced editor and withdrawal
command; it is not a news item or a blotter message.
