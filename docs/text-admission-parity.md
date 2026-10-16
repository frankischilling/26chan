# Text admission and formatting

Issue #208 covers posting text, display names, trips and saved formatting.
The implementation includes the staff image-only OP exception, fixed name/trip
checks for staff posts and source greentext prefix recognition. The final PR
head must pass the full native CI suite before merge.

## Staff image-only threads

The pinned source's final admission block accepts a blank subject and comment
only when a moderator has an image and a nonempty Options field. The source
captures Options after removing `sage` and before removing a capcode. A selected
staff badge therefore counts as a nonempty Options field in the separate staff
form. An ordinary staff post with only `sage` does not qualify; `nonoko` does.
Janitors do not receive this exception.

The earlier required-subject check still runs. The later text-only subject check
belongs to the source's nonblank-comment branch. Board attachment policy is
checked separately, and the image-only exception does not authorize an upload.
The existing staff receipt binds the post and checks the authenticated role,
board scope and attachment capability before anything commits.

`extract-post-content-admission-reference.py` executes only the pinned final
admission block with synthetic arguments and two terminal stubs. Its 192 cases
cover position, subject, blank/nonblank text, image presence, Options, moderator
status and text-only policy. A separate domain matrix covers the earlier
required-subject rule. The actual staff attachment test checks badged and
ordinary moderator, manager, administrator and janitor paths, rejected writes,
unchanged upload receipts and committed image-only threads.

## Greentext and saved profiles

The source protects `>>` followed by digits and `>>>/board/` with a lowercase
ASCII-alphanumeric board before adding green quote spans. It does not exclude
all text starting with two greater-than signs. New posts render `>>not-a-post`,
`>>>42` and malformed board prefixes as greentext. A syntactically protected
reference stays uncolored even when its destination cannot be resolved.

Migration 0175 stamps profiles 232–239 and 248–255 on new posts. Existing profiles
keep their previous rendering. The new profiles retain the spoiler, code, SJIS,
OP-markup, wrapping and source-link bits. Ordinary comments and saved wordfilter
results use the same prefix decision. The public HTML, JSON, updater, catalog,
archive and frozen-transfer paths recognize the new profiles. Moderation
snapshots accept and retain them.

The greentext extractor executes the exact pinned greentext pass after explicit
synthetic wrapping and line breaks. Its 64 vectors include numeric and malformed
prefixes, unknown boards, zero/one/two spaces after a break, and the 35-character
wrap boundary. These vectors do not claim to re-execute linkification or BBCode;
those have their own source fixtures. Tests compare the vectors with every new
profile in both plain and saved-wordfilter paths. Historical-profile controls
and frozen-transfer round trips check that old posts do not change.

## Fixed name and trip checks

The source runs its fixed name/trip hook for badged staff as well as ordinary
posts. Configured public content filters remain conditional, but the fixed hook
now runs in both paths. Its moderator exception is based on server-owned role
authority, including an unbadged moderator. Janitors retain ordinary behavior.
Native normalization errors fail closed before a staff proof or upload is spent.

The hook receives escaped display text and an already-derived normal trip.
When a badge masks the name, the derived normal hash survives only for this
in-memory check; the saved name is `Anonymous` and no trip is stored.
Suppressed trip passwords and modern secure-trip secrets never enter this
check. The source sometimes inspected those raw secrets; the rewrite retains
the documented security exception in compatibility item E-018. Production's
unrecorded `mb_internal_encoding` is also an explicit source-input limit, not an
inferred deployment setting.

## Existing qualification

The implementation uses the earlier source-derived checks rather than a second
parser or a second name pipeline:

- [Comment cleanup and spacing](source-comment-spacing.md),
  [subject cleanup](source-subject-cleanup.md),
  [required subjects](source-required-subject.md) and
  [admission hooks](verification-content-admission.md)
- [Public names and CP932 trips](verification-public-names.md),
  [trip suppression](verification-trip-suppression.md),
  [staff identities](verification-staff-identity.md) and
  [ordinary staff posting](staff-posting-authority.md)
- [BBCode](source-comment-markup.md), [OP markup](source-op-markup.md),
  [soft wrapping](source-word-breaks.md), source links and quote resolution

Secure trips use deployment-owned keys, not the original private salt. User HTML
remains escaped and generated markup remains typed. Source configuration and
encoding inputs are pinned in their respective fixtures; missing private salts
are not reconstructed. Historical rows are not rewritten.

## Deployment

Pause posting while applying migration 0175 and deploying a binary that
understands the new profiles. Resume writes only after both are in place. Older binaries do not understand these new stamps; rolling
back only the binary is not a rendering-compatible rollback for posts created
after this migration. No migration backfills existing posts or audit snapshots.

## Reproduction

With the disposable development database and runtime credentials loaded:

```sh
cargo test -p board-domain --locked
cargo test -p board-store --all-features --locked --test staff_post_attachments --test moderation_target_snapshots --test thread_transfer_authority
cargo test -p board-public --all-features --locked --test comment_format --test op_markup --test post_content --test word_breaks --test server_links --test wordfilters
sudo bash scripts/test-greentext-migration.sh
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

The upgrade test seeds existing posts and nonempty audit snapshots before 0175,
then compares complete saved rows after migration. It checks every new policy
stamp through the real trigger and verifies transfer readiness. Domain tests
also cover old-profile rendering and frozen plain/wordfiltered presentations.

The source fixtures can be regenerated only with the pinned checkout:

```sh
python3 scripts/extract-post-content-admission-reference.py 4chan-old crates/domain/tests/fixtures/post-content-admission.json --check
python3 scripts/extract-greentext-reference.py 4chan-old crates/domain/tests/fixtures/greentext.json --check
```
