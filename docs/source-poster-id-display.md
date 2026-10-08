# Source poster-ID display policy

Ordinary sage posts save `Heaven` on ID-enabled boards unless META_BOARD or
DISP_ID_NO_HEAVEN suppresses that label. Other ordinary posts keep their scoped
network label. Badged staff posts keep the separate static staff label; sage
and the two suppression switches do not change it. Boards without IDs save
no label on new posts.

The supplied global setting disables DISP_ID_NO_HEAVEN. The `/bant/`, `/biz/`,
`/pol/`, `/qst/` and `/soc/` configurations enable it. Commented assignments
on `/his/` and `/test/` do not override the global setting. Migration 0074
stores this policy as `content.boards.poster_id_no_heaven`, with a false default
for additional boards. META_BOARD remains an independent setting.

The posting transaction supplies its parsed sage decision. The insertion
trigger locks and reads the board policy after staff badge authority is
consumed. Ordinary posts still supply a valid eight-character network label;
the trigger then selects `Heaven` when eligible. Missing keys or peer identity
retain their existing failure behavior. Public callers cannot supply a short
label through the poster-ID GUC or write the saved column directly.

The private full-digest poster context remains separate from the visible
label. Two peers using `Heaven` still count as two posters. Saved fields are
not recomputed when a board setting changes. Replies expose saved fields in
JSON and HTML; archive JSON omits IDs. The source JSON helper separately
recomputes the ordinary OP's network label, including when its saved HTML
label is `Heaven`.

Migration 0075 captures that already-derived public network label in
`content.posts.json_op_poster_id` when inserting an ordinary, ID-enabled OP.
Both JSON listeners use it for the OP while HTML uses the saved display label.
Replies and badged posts leave the new field null. The field is never exported
under its internal name. Runtime roles cannot insert or update it directly;
its constraint permits only an eight-character label on an ordinary OP.
No raw address or private fingerprint is stored in this column.

Neither migration backfills historical posts. An older ordinary OP's saved
eight-character label remains a valid JSON fallback. A historical `Heaven` OP
without a captured network label omits its JSON ID because its original
address was not retained. This is a migration limit, not an exact source
projection for that historical case. Both upgrades preserve prior post and
thread fields, posting proofs, privacy policies, function OIDs, owners and
grants.

Migration 0076 grants read access to `meta_board` and `poster_id_no_heaven`
to the existing NOLOGIN attachment function owner. Attachment insertion runs
under that owner and needs the same policy read as plain posting. Runtime
grants, policy write access and the function's metadata remain unchanged.
Public readiness verifies these reads before accepting traffic.

The reference pins `imgboard.php` at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`, SHA-256
`caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`.
The extractor evaluates only the pure staff-label and ordinary fallback blocks
with synthetic inputs. Its network generator is a fixed eight-character stub;
it does not evaluate the source's network hashing or application runtime.
PHP runs with a 16 MiB memory limit and five-second deadline. Output is bounded
to 16 KiB and the saved fixture to 32 KiB. The 112 cases cover all six badges
and ordinary posts across ID, sage, META_BOARD and no-Heaven switches.

The JSON reference additionally pins `json.php`, SHA-256
`18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868`,
at the same source revision. Its extractor evaluates only the pure ID
projection block with synthetic inputs and the same network-label stub.
The 448 cases combine every display case with OP/reply and live/archive
states. PHP retains the same memory and time bounds; output and fixture
are each bounded to 64 KiB. The Rust projection test compares every case
and covers historical fallback and missing-network behavior separately.

The populated 0074 upgrade compares all 112 inserted labels with the source
fixture. The 0075 upgrade checks 112 OP and 112 reply insertions, captures
network labels only for eligible ordinary OPs and consumes a real posting
proof prepared before the upgrade. Both check historical fields, privacy
policies, function metadata, owners, grants and proof replay denials.
The HTTP regression covers the 16 ordinary reply and 16 ordinary OP
combinations, both JSON listeners, HTML, complete private poster counts,
policy changes, archives and an actual board-lock wait. The existing staff
posting suite covers static badge IDs.

The native color and click controllers accept the exact `Heaven` label beside
the ordinary eight-character alphabet. Near matches and staff labels in an
ordinary ID span remain rejected. The released client hash gives `Heaven`
the color `rgb(128, 154, 18)` with black text. Browser checks cover dynamically
inserted labels, loaded-post counts and keyboard highlighting. The public
browser fixture covers both network and sage posts through previews, updates,
color settings and ID filters.

```text
python3 scripts/extract-poster-id-display-reference.py SOURCE fixtures/poster-id-display-reference.json --check
python3 scripts/extract-poster-id-json-reference.py SOURCE fixtures/poster-id-display-reference.json fixtures/poster-id-json-reference.json --check
bash scripts/test-poster-id-heaven-migration.sh
bash scripts/test-json-op-poster-id-migration.sh
bash scripts/test-attachment-poster-id-policy-migration.sh
cargo test -p board-public --test poster_ids --test poster_counts --test board_inventory --all-features --locked
```

Apply migrations 0074 through 0076 before running the updated public application;
its readiness check requires both new columns and the attachment-owner reads. See
[verification](verification-poster-id-display.md) for results and limits.
Unbadged authorized posting, remaining source admission/option rules and the
complete board, media and frontend scope remain unfinished.
