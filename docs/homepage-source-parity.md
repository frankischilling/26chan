# Homepage and information-page source comparison

Issue [#194](https://github.com/frankischilling/26chan/issues/194) covers the homepage, directory, local information pages and blotter. The local draft adds grouped board discovery and an introductory homepage layout alongside the existing blotter. Local information pages and operator-maintained news now have a storage and rendering implementation under qualification. Frontpage content/statistics and source posting notices remain unfinished; this draft does not close the issue.

## Available evidence

`fixtures/homepage-reference.json` records hashes of the supplied board directory, frontpage/statistics producers, blotter view and news archives. They belong to original commit `545b7812d1849f7958d914950c91fdbbe38f6b22`. The supplied tree does not contain the homepage controller/template, rules or FAQ content, news controller, blotter controller, Atom generator or source database contents.

The [public homepage](https://www.4chan.org/) observed on October 15, 2026 supplies a secondary directory-grouping reference. Its rendered text lists seven topic groups, popular threads, statistics and information-page navigation. This observation is newer than the supplied source and is not a historical HTML or pixel baseline. The fixture records only group membership facts; source-board names and classifications remain tied to the supplied directory. The current observation omits original directory members `/asp/`, `/r/` and `/trash/`; those boards must not disappear from the rewrite. Source entries absent from the secondary grouping and additional local boards need a separate fallback group, while private boards remain excluded.

The current [FAQ](https://www.4chan.org/faq) and [rules](https://www.4chan.org/rules) are navigation references, not proof of the missing historical pages. Local help must explain the implemented runtime accurately, and operator-maintained rules must not silently import another site's legal identity, contacts or payment arrangements.

## Original frontpage producer

`update_fpcontent.php` produces recent images, latest posts, popular threads and aggregate statistics. It excludes `/b/`, `/f/` and `/qa/` from promoted content after collecting statistics. Latest-post and image candidates are older than ten minutes, with five candidates per board in each query. A reported post, a subject starting with the source spoiler marker, a comment containing `[spoiler]`, or a filename containing `.pdf` or `.webm` is excluded. Popular threads additionally need at least fifteen replies and a root/bump value newer than ten minutes ago. The source encodes sticky roots in January 2027 (`admin.php:3560-3580`), so sticky threads can pass that condition without a recent ordinary bump. These are literal producer conditions; the missing consumer may apply further ordering or limits.

Source totals sum adjusted maximum post numbers, file sizes and distinct hosts for each board. The old skipped-ID corrections are historical counter repairs, not counts of this installation's posts. The rewrite must not publish another site's totals or expose private poster identifiers. Any replacement activity metric needs an explicit definition and privacy boundary.

`update_board_stats.php` periodically appends each board's maximum post number and the collection time. Failed board reads stop that update. Empty boards are skipped. A local equivalent needs empty/stale behavior and bounded retention rather than an unbounded history table.

## Implementation boundaries

Local homepage data should come from bounded public projections. Private boards, unpublished or deleted attachments and unsafe promotion candidates must not enter that projection. Rendering must escape operator-controlled content and use local board, news, rules and FAQ destinations. News needs publication and retraction behavior; a hard-coded historical archive does not provide an update workflow.

The already merged blotter retains newest-three board previews, explicit board visibility policy, timestamp-based dismissal and a paginated local listing. Homepage work must preserve those contracts. Missing source templates and generators remain listed above so visual or feed behavior is not presented as verified without evidence.

## Draft checks

The actual Askama homepage template and grouping module compile in an isolated offline harness with a reduced board model. It checks 79 public fixture entries, 25 classifications including a custom board, escaped operator labels and the empty state. This is a focused template check, not a full application or browser pass. The full application test cannot run in the local offline cache because `rustls` 0.23.46 is absent; hosted qualification remains required.

`fixtures/homepage-promotion-reference.json` captures the exact pinned `check_post` helper and fourteen literal cases. `scripts/test-homepage-promotion-reference.py` executes it with a database double and no database access. This preserves case-sensitive filename/marker behavior, substring exclusions, reported-post rejection and healthy ordinary controls. The local PHP oracle passes; the runtime promotion projection is still pending.

The [local content workflow](local-site-content.md) covers page revisions, news publication/retraction, plain-text rendering and read-only public projections. It is a security-bounded replacement for the source's trusted HTML news fragments; historical images and external identities are not imported. Full application and browser qualification remain pending.
