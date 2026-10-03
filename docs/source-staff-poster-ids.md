# Source static staff IDs

Badged posts on boards with `user_ids` enabled save the source's static public
ID label. The badge is already validated by the consumed staff posting proof.
It needs no network address or `POSTER_ID_KEY`.

| Saved badge | Saved public ID |
|---|---|
| `mod` | `Mod` |
| `admin` | `Admin` |
| `admin_highlight` | `Admin` |
| `manager` | `Manager` |
| `developer` | `Developer` |
| `founder` | `Founder` |

Boards without IDs save no label on a new staff post. Changing the setting
affects future posts; existing labels and null fields remain unchanged.
Migration 0073 performs no backfill or content update. Historical
eight-character IDs remain valid, including those on a badged historical row.

Full, index, catalog and selected tail replies expose the saved label in JSON.
Tail OP metadata stays minimal. Archived JSON omits IDs, while the database
retains them. Forced-anonymous and meta-board name/trip masking follows the
separate [JSON rules](source-staff-json.md); it does not recompute the ID.
HTML headers keep the badge and omit `posteruid`, as in the supplied source.
Static labels add no network fingerprint or complete private poster count.

## Authority and migration

The insertion trigger runs after current badge authority is consumed. It locks
the board's ID setting and derives the label from that validated badge. Public
GUCs cannot grant a badge. A short static label also must match the saved badge
under the database constraint; ordinary posts still require their existing
eight-character ID shape. Runtime roles cannot insert or update `poster_id`
directly or execute its trigger function.

Migration 0073 preserves the trigger function's OID, owner, execution policy
and grants. It changes no privacy policy, board setting, posting proof, saved
post or thread. Apply it before running the updated posting service. An existing
source-bound proof retains its exact name, trip, badge, body and expiry checks;
the migration grants no new authority to that proof.

## Source evidence and checks

The pure `$ma`/`$uid` block comes from `imgboard.php` at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`, SHA-256
`caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`.
The extractor parameterizes only `DISP_ID` and runs the bounded block with
synthetic badges. It loads no application, configuration, session or database.
Its 14 cases cover six badges with IDs on/off and two `none` controls. Those
controls produce no static staff label; ordinary network IDs use their separate
posting path.

The populated upgrade checks exact historical rows and proof fields, privacy
policies, function metadata, owners and ACLs. It consumes a proof issued before
the upgrade, checks its prepared trip and static ID, rejects replay and public
label forgery, and checks that no poster context was created. The HTTP
regression checks every badge and display setting through real staff posting,
both JSON listeners, policy changes, archives and HTML headers.

```text
python3 scripts/extract-staff-poster-ids-reference.py SOURCE apps/staff/tests/fixtures/staff-poster-ids.json --check
bash scripts/test-staff-poster-id-migration.sh
cargo test -p board-staff --test posting --all-features --locked
```

Execution results are recorded in [static staff ID qualification](verification-staff-poster-ids.md).
Complete source JSON, unbadged authorized posting, Pass benefits and remaining
board, media and frontend behavior still require the full rewrite work.
