# Text-only board policy

[Issue #147](https://github.com/frankischilling/26chan/issues/147) follows the
supplied source's `TEXT_ONLY` policy. `config/global_config.ini:126` defaults
it off; the active `news.config.ini:11` enables it. Migration 0033 updates
existing `/news/` and leaves other and future boards disabled. Operators must
set source overrides when creating a board after migration; the synthetic news
fixture does so. Historical posts and earlier policy fields remain unchanged.

The ordinary posting handler checks these source rules:

- `imgboard.php:4866-4868` rejects an attachment on a reply with
  `You cannot upload files on this board`. Intake checks the parent and board
  before reading file bytes or creating a job. Posting rechecks committed
  policy under the board lock. Rejected requests do not consume approvals.
- `imgboard.php:5790-5802` checks the subject after final markup admission.
  A wholly empty OP returns the ordinary subject-or-comment error; a nonempty
  comment with an empty cleaned subject returns the subject-required error.
  Earlier explicit `REQUIRE_SUBJECT`, raw limits and line checks retain their
  order. Replies with text need no subject.
- `views/imgboard.php:91-104,154` requires the OP subject in the native form
  and hides upload controls. Board, thread, catalog and archive pages carry
  the source `text_only` body class. The source JSON builder at
  `imgboard.php:7915-7925` emits integer `text_only` and `require_subject` flags.

The active handler does not reject an OP file solely because `TEXT_ONLY` is
enabled. The API or direct approved-attachment path can therefore accept an OP
file with valid content and authority even though the native form hides the
upload control. `TEXT_ONLY_ALLOW_OP` appears in an alternate test template;
the active template and handler do not use it. This implementation preserves
that distinction rather than adding an unsupported rejection rule.

Development media availability remains a separate condition. With processing
disabled, uploads are still unavailable and the API retains its existing
media-disabled `text_only` advertisement. That advertisement alone does not
enable source subject requirements. Production media stays disabled pending
qualification.

## Database and rollout

Apply migration 0033 before deploying the binary. A caller cannot bypass the
reply attachment rule through `content.insert_post_attachment`: an invoker
trigger on the final attachment row checks the locked board policy. The
attachment owner gains only a read of the new non-private column. Public and
staff roles receive no policy-write, function-execution, trigger or schema
authority. The trigger rejects new associations; it does not erase historical
attachments when an operator changes policy.

Keep the additive column and trigger on binary rollback. An older binary may
display an upload control or lack the new subject check, but the database still
rejects reply attachments. Releasing an older binary therefore loses some
source behavior and is not a complete policy-preserving rollback. Restore
verification compares complete board-policy rows as well as post content.

## Verification

Domain cases cover final admission and error precedence. Actual database tests
cover both form encodings/routes and response modes, both JSON listeners,
historical posts, policy-lock witnesses, denied public updates, approved OP
attachments, blocked replies, direct scoped-SQL rejection and approval reuse.
The intake test supplies the parent field while file bytes remain pending and
requires rejection within two seconds without creating a job. Multipart may
poll the pending stream while parsing the parent; that is not a file-byte read.
The same healthy intake service subsequently handles an allowed upload.

`scripts/test-text-only-migration.sh` upgrades a populated 0032 database with
all 82 active source board names, checks defaults and historical rows, and
executes allowed reads and denied runtime policy/schema changes. Browser tests
exercise `/news/` with JavaScript enabled and disabled. Separate desktop/mobile
fixtures render the real templates with media configured and confirm hidden
upload controls, subject validity and bounded page width.

Local domain tests and strict domain/store/public Clippy with all targets and
features passed. Shell syntax, JavaScript syntax and actionlint passed.
Desktop (1280px) and mobile (390px) fixture tests passed, and both captured
pages were visually inspected. Existing screenshot baselines were unchanged.
The old local database environment timed out. After loading the current owned
native PostgreSQL environment, migrations and the focused markup-admission,
required-subject, text-only and actual intake tests passed. Populated upgrade,
restore and complete hosted qualification remain required before merge. These
checks do not establish complete source parity or deployed media containment.
