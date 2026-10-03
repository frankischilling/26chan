# Public posting spoilers

New public posts honor the board's imported `SPOILERS` policy, represented by
`comment_spoiler_cleanup`. Enabled boards accept the selected flag with or
without a file. Disabled boards ignore the flag. Approved-image forms and
Quick Reply offer the checkbox only when the policy enables it. A forged
field cannot override the server's locked policy.

The saved flag is independent of the subject and attachment. Public thread,
tail and catalog JSON emit `spoiler: 1` when that flag is set, including text
posts and posts whose file was deleted. File deletion still removes the
download metadata and revokes reader access. A literal subject beginning
`SPOILER<>` remains user text. Turning off the board policy does not rewrite
historical spoiler choices.

`boards.json` advertises `spoilers: 1` only when the board policy enables it,
including when its image cap is zero or the media service is unavailable.
Policy advertisement is independent of attachment availability, as in the
source `get_catalog_info()` projection. Custom spoiler counts remain unfinished.

## Source comparison

The reference is the supplied checkout at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The
[extractor](../scripts/extract-public-spoiler-reference.py) pins `imgboard.php`
and `json.php`, executes the actual POST scalar extraction, spoiler-prefix
preparation, JSON prefix decoding and board policy projection under PHP, and records
[144 synthetic cases](../apps/public/tests/fixtures/public-post-spoilers.json).

These cases cover both policies, nine omitted/scalar choices, four prepared
subjects and attachment presence. Source PHP treats an omitted field, empty
string and `"0"` as false; `"false"`, `"00"` and other nonempty scalar values
are true. The Rust spoiler field uses those scalar semantics. Other typed
form fields keep their validation. Arrays, duplicate fields, unknown controls
and unbounded requests do not acquire source global-variable authority.

The fixture starts with prepared, escaped subjects, a synthetic attachment
bit and a zero custom-spoiler count. It does not execute the original database,
full posting endpoint, formatter or page renderer. Real Rust handler submissions separately compare
saved flags, subjects, attachment state, upload-form availability and both
public JSON origins against every recorded case. The attachment cases also
check JSON after file-only deletion.
Board-directory comparisons cover both public JSON origins, zero/nonzero image
caps and media-enabled/disabled routers against the executed source projection.
Those added handler assertions still await a completed run after the directory
advertisement correction.

The source form in `views/imgboard.php` conditions its spoiler controls on
`SPOILERS`; the source extension in `js/extension.js` creates its Quick Reply
control only when the source form contains that checkbox. The rewrite carries
the board policy into the isolated upload form and native Quick Reply. Custom
spoiler assets and the remaining specialized media forms remain separate work.

## Storage and operation

Apply [migration 0080](../migrations/0080_public_post_spoilers.sql) before
starting the updated binary. It adds two private invoker trigger functions
with a fixed search path and initializes new records. It changes no existing
rows, grants, owner roles, earlier functions or privacy policies. Published
migrations 0077, 0078 and 0079 remain unchanged.

The public post trigger accepts the transaction's raw new-post choice only
for the actual public login and checks the locked board policy. The attachment
trigger independently applies that policy inside the existing one-use upload
entry point. A direct SQL caller supplying true on a disabled board cannot
bypass the attachment rule. Existing-post changes retain the separate
[staff spoiler setter](staff-image-spoilers.md). No public runtime can write
the state columns or execute that setter.

The new public choice is not added to ordinary staff proof metadata. Staff
posting rejects that raw metadata choice, and its existing proof validation
is unchanged. Ordinary staff attachments and their complete source form remain
unfinished. Staff spoiler actions continue to control existing text, image
and retained archive posts.

The [populated upgrade](../scripts/test-public-spoiler-migration.sh) compares
historical rows, privacy policies, existing functions and all selected
table/column grants in both directions. It connects as each of the seven real
runtime logins to deny direct state writes and trigger execution, then uses
the actual public login to insert enabled and disabled text choices. Fresh
bootstrap also denies runtime execution of both triggers.

The separate local integration database has 71 migration files through
version 80. Its qualified version-79 parent is unchanged. During binary
rollback, keep the additive schema and historical state. An older binary
does not provide these form, scalar or JSON semantics; no destructive down
migration is supplied.

## Verification

The source fixture regenerates exactly. The 144 handler comparisons, the
observed board-lock policy wait, existing native/upload integration tests,
attachment integration suite, fresh bootstrap and populated upgrade pass
locally. The direct attachment SQL comparison deliberately bypasses Rust
normalization while retaining its actual one-use capability. Strict workspace
Clippy across all targets/features passes with warnings denied. Workspace
binaries/examples build, all 45 store tests pass on a fresh migrated/seeded
database, and 16 JavaScript tests pass with the pinned browser installed.

The first matrix run reached the normal shared-peer request budget. It now
uses an explicit fixture budget; runtime defaults and their HTTP-limit tests
are unchanged. An older upload assertion dropped `spoiler` after file deletion;
it now checks that source state survives while file metadata disappears. Both
failed logs are retained locally.

The full fresh public/staff run remains incomplete. WSL restarted during
compilation and again during public tests, including with a verified 2 GiB
build scope. Interrupted executables were preserved before rebuilding. A
separate public attempt failed because the local helper omitted Node; its
pinned tool path is now restored. The available logs do not establish the
restart cause, and no interrupted run counts as a pass.

Full fresh public/staff regressions remain unqualified. The browser run passed
both existing staff workflows, but its new spoiler case timed out after using
the desktop-only Quick Reply link at 390 pixels. It now uses the existing
mobile post reply control at that width. The corrected full rerun and a
separate fresh staff build were interrupted by another WSL restart, before
they produced a result. The failed and interrupted logs remain local.

The new browser workflow checks disabled/enabled Quick Reply controls on
desktop/mobile and forged text choices. Its complete pass is still required.
The media qualification adds enabled/disabled approved and inline Quick Reply
uploads through the existing real isolated pipeline. Synthetic coordinator approvals in the handler tests do not
establish decoding or worker isolation. No complete hosted pass, deployment
or full-source completion is claimed.

[Build/test at 0827c24](https://github.com/frankischilling/26chan/actions/runs/37152385639)
failed. Linux's upload-browser fixture and four Windows Quick Reply cases
expected spoiler controls while configuring a disabled board. Windows passed
64 attachment cases before that failure; the proposed theme shards did not
run. The real media pipeline reached its new disabled-board normal-file case,
then expected a popup from a thumbnail click. The enabled spoiler samples now
explicitly enable their board policy and carry the matching saved post flag.
The normal-file popup check uses its download link, retaining the existing
thumbnail behavior, popup assertions, deadlines and baselines. Public upload
browser checks now also require saved spoiler state after file deletion.
These corrections still require a complete current-head run.

Native Windows public Clippy across all targets/features passes with warnings
denied after the directory/fixture corrections. Its initial example build
lacked the pinned ICU import library. After verifying the existing ICU 74.2
development dependency, compilation failed with Windows error 1455, "The paging
file is too small," and failed allocations. The local follow-up handler build
also ended with another WSL restart. Native build, handler and browser checks
remain unqualified; their failed/interrupted logs are retained.

The Windows runs at `a804708` and `7d5b16b` each passed 1,172 theme cases and
failed one. Their retained traces record `ERR_NO_BUFFER_SPACE` on the theme
stylesheet and page-chrome module respectively. The resource aggregates do
not establish the cause. CI now partitions all theme cases into four
sequential shards, starting a fresh browser/server pair for each. Assertions,
retries, deadlines and screenshot baselines are unchanged. This limits each
process lifetime. An independent test enumeration compares the complete suite
with the four shards: all 1,173 cases occur exactly once, distributed as
475/264/384/50. The change remains a proposed qualification-environment correction
until the current-head run completes.

```text
python3 scripts/extract-public-spoiler-reference.py 4chan-old \
  apps/public/tests/fixtures/public-post-spoilers.json --check
cargo test --locked -p board-public --all-features --test spoiler_policy
sudo bash scripts/test-public-spoiler-migration.sh
```
