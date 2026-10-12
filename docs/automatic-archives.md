# Automatic archives at the reply threshold

The supplied source enables `AUTOARCHIVE_CAP = 1500` on `/jp/` and `/vt/`.
Its global default is zero. After accepting a reply, `imgboard.php:6726-6730`
archives an ordinary thread when its surviving reply count reaches the cap.
Sticky and undead threads are excluded independently. The counter at line 9763
counts replies, excluding the opening post.

Migration 0137 imports those two switches without archiving existing threads or
rewriting posts. Other boards retain a zero cap. `reply_limit` stays unchanged,
including the separate sticky/undead window. On an ordinary thread with archives
enabled, posting admits the larger of `reply_limit` and `autoarchive_cap`, then
archives at the latter threshold. Set the cap to zero to retain only the existing
reply-limit policy. Runtime roles cannot change either setting.

## Transaction and reader behavior

The final reply and archive transition commit together under the existing board
and thread locks. Rejected content and Robot9000 attempts do not archive a
thread. A failure during archival rolls back the new reply and its deletion
state. Concurrent final replies serialize: the first successful transition
prevents the next reply from entering the archived thread.

The threshold uses surviving rows, so deleted replies do not consume it. The
cached count follows that same count on ordinary autoarchive threads. Archival
resets the root/bump clock to archive time, as the source does, while preserving
the accepted reply's source modification time. Existing archive triggers retire
deletion hashes and apply the report-group threshold rules. Public JSON exposes
`archived`, `archived_on` and `closed`; the native-client transport keeps
its separate staff-closed and archived flags.

Thread storage, complete reads, API/tail responses, native snapshot validation,
expansion and spoiler-reveal counts accommodate 1,500 replies plus the OP.
Existing byte, node, response and deadline budgets remain in place. An oversized
thread can still fail a bounded read; increasing cardinality does not disable
those checks. The upload preflight and displayed ordinary reply ceiling use the
same board policy as posting.

## Upgrade and verification

Deploy migration 0137 before the updated services. Once threads can exceed
1,000 replies, older binaries cannot serve them completely; do not roll back to
an old reader while retaining those threads. The migration preserves the
text-transfer catalog guard: it rejects pre-existing drift before resealing its
reviewed schema change. No runtime receives resealing authority.

The source recorder checks the supplied `imgboard.php` hash and both board
settings. Its 448 vectors record the boolean predicate and threshold boundaries;
these are extracted-contract tests, not an execution of the original server.

- `cargo test -p board-store --features database-tests --test autoarchive --locked`
  exercises actual public-role posting, protected/disabled cases, deleted replies,
  rejection, a late archive failure, concurrent final replies and the 1,500 limit.
- `cargo test -p board-public --features database-tests --test autoarchive_api --locked`
  submits the final anonymous reply through the HTTP posting handler, then checks
  both JSON listeners, complete/tail responses, validators, HTML and the native
  updater response.
- `node --test tests/browser/native-updater.test.mjs` checks full and tail native
  snapshots at the new limit, including rejection above it.
- `sudo bash scripts/test-autoarchive-migration.sh` checks a populated upgrade,
  preserved operator settings, transfer-catalog drift and runtime policy denial.

## Related lifecycle work

This implements the automatic reply-threshold portion of issue #215. Current
age policy, count-independent storage and historical archive credentials are
covered in the [lifecycle comparison](archive-lifecycle-audit.md). Staff text transfer keeps its
existing 1,001-post admission bound. Full-page historical visual identity and
production deployment qualification are not established by these tests.
