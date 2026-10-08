# Source custom-spoiler metadata

The public JSON interface now carries the source `SPOILER_NUM` policy.
Enabled boards advertise `custom_spoilers` in `boards.json` when the count is
nonzero. Their opening posts carry `custom_spoiler`, including zero, in full
thread, tail-thread, index and catalog JSON. Replies omit that field. Disabled
boards omit both fields. Image caps, configured media availability and
attachment presence do not determine these policy fields.

This is the metadata part of #217. [Spoiler rendering](custom-spoiler-rendering.md)
describes the imported HTML thumbnail policies, catalog suffixes, native cache
and collected UI images. Two spoiler images and test flag artwork remain unavailable.
[The source flag inventory](source-board-flags.md) records all 165 definitions.
Full historical visual parity is unproven.

## Source comparison

The supplied checkout is revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`.
[The recorded fixture](../apps/public/tests/fixtures/custom-spoilers.json)
includes the existing hashes for every board/category/global configuration,
plus `lib/ini.php`, `imgboard.php`, `json.php` and `catalog.php`.
[The extractor](../scripts/extract-custom-spoiler-reference.py) runs the actual
INI parser/evaluator and the selected board, full/tail OP and catalog
projection blocks under PHP 8.3.6. It executes all 82 configurations and eight
explicit enabled/count policies. Deterministic seeds enumerate the finite thumbnail
URLs evaluated by the source; they do not establish historical pixel identity.

Nineteen board configurations have nonzero counts: `/a/`, `/m/`, `/v/`, `/co/`,
`/jp/`, `/mlp/`, `/tg/`, `/tv/`, `/lit/`, `/vp/`, `/vg/`, `/vr/`, `/s4s/`,
`/news/`, `/vrpg/`, `/vmg/`, `/vst/`, `/vt/` and `/vm/`. Their counts range
from one to six. The fixture also retains the source HTML aliases used by
`/news/`, `/vm/` and `/vst/`.

The comparison uses synthetic OP/reply identifiers and a banned-post
projection bit. It does not run the complete source endpoint, formatter,
database or cross-board renderer. Thumbnail files are not bundled in the
supplied checkout.

## Migration and authority

Apply [migration 0081](../migrations/0081_custom_spoiler_counts.sql) before
starting the updated binaries. It adds `custom_spoiler_count`, defaulting to
zero, with a 0-64 bound, then imports the 82 source counts. It preserves
existing spoiler enablement, operator-created boards, posts, attachments,
timestamps, deletion records, audits, functions, privacy policies and runtime
grants. Runtime roles gain no policy-write authority.

[The populated upgrade check](../scripts/test-custom-spoiler-migration.sh)
compares historical records and authority in both directions, excluding only
the new board column from the historical row comparison. It keeps a deliberate
operator override of `/a/` spoiler enablement, checks defaults and invalid
counts, then denies inserts and updates from seven actual runtime logins. Its
first local attempt omitted LOGIN on the disposable staff/auth/reader/intake/
monitor roles; the fixture now enables those test logins without changing the
production bootstrap or migration. The failed log remains local.

The separate local integration database has 72 successful migration files
through version 81. Its qualified version-80 parent and the published
0077/0078/0079/0080 checksums are unchanged. Binary rollback keeps the additive
schema. An older binary omits these fields; no destructive down migration is
provided.

## Verification

The fixture and migration regenerate exactly. The populated upgrade and fresh
role bootstrap pass. Native Windows strict public Clippy passes across all
targets/features with warnings denied. Its library and focused integration
run passes 102 tests: 96 library tests, the new custom-count test, both existing
spoiler-policy tests, the existing tail-response contract, the upload browser
integration and the word-break policy change. The Linux kernel-peer library
case is absent on Windows.

[The handler integration](../apps/public/tests/custom_spoilers.rs) checks all
82 imported counts and the eight explicit source policies against both JSON
origins, with zero/nonzero image caps and media-enabled/disabled routers. It
checks board, full/tail thread, index and catalog projections, text-only
opening posts, reply omission and seven runtime write denials. These router
settings do not establish decoding or worker isolation.

The tail regression first failed because an enabled board's opening tail
record omitted `custom_spoiler: 0`. The corrected projection passes the full
policy matrix, including nonzero counts and disabled-board omission.

Linux strict workspace Clippy passes across all targets/features, and all
workspace binaries/examples build. The same public selection passes 103 tests,
including the Linux kernel-peer case. All 45 store and 77 staff tests pass on
Linux; the native Windows store run also passes all 45 tests on a fresh,
migrated and seeded disposable cluster. Both forced-anonymous browser cases
pass on Linux and Windows, including real Quick Reply submission. The Linux
run took 2.9 minutes, with 2.3- and 3.4-second test bodies. Earlier
WSL-interrupted attempts remain retained.
One final local helper launch exited before checks because Windows had written
CRLF shell line endings; the corrected helper runs with LF endings in the
verified 3 GiB build scope.

Current-head hosted qualification and custom-spoiler browser/asset parity
remain required. Results from earlier commits apply to those recorded commits,
not to this additive schema change.

The anonymous-session expiry fixture now computes inactivity from the
application clock. Robot9000 fixtures use whole-second expired mutes beyond
the accepted 30-second application/database clock tolerance. Both changes
keep the fixtures consistent with the existing policies. The corrected
forced-anonymous browser cases wait for the mounted Quick Reply control
before clicking a reply number and retain the original identity and
submission assertions.

```text
python3 scripts/extract-custom-spoiler-reference.py 4chan-old \
  apps/public/tests/fixtures/custom-spoilers.json \
  --migration migrations/0081_custom_spoiler_counts.sql --check
cargo test --locked -p board-public --all-features --test custom_spoilers --test thread_tails
sudo bash scripts/test-custom-spoiler-migration.sh
```
