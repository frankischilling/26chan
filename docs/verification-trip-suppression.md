# Source trip suppression

Ordinary posts on `/b/` and `/s4s/` retain the prepared display name and discard
both normal and secure trips. `Name#password` saves `Name`; `#password` saves
`Anonymous`. Secure syntax needs no deployment key on a suppressing board.
Saved identities remain visible when the policy changes. This completes the
ordinary `STRIP_TRIPCODE` portion of issues #190 and #208; staff preparation
and privileged exceptions remain unfinished.

## Source evidence

The supplied checkout is pinned at
`545b7812d1849f7958d914950c91fdbbe38f6b22`.
`extract-trip-policy-reference.py` verifies every configuration hash against
the existing board inventory, then resolves active inherited settings. The
global value is `no`; only `b` and `s4s` override it to `yes`. Commented
declarations do not override it. All 82 definitions, including the two private
definitions, are recorded in `fixtures/trip-policy-reference.json`; 80 are
listed publicly. Extraction also checks the generated migration byte for byte
without changing the earlier board-import migrations.

The bounded name extractor evaluates the pinned public name block with seven
synthetic policies and 469 cases. The two additional profiles reproduce the
actual suppressing boards. They retain whole-field cleanup, CP932 conversion,
delimiter parsing and escaped-name bounds while skipping hashing, secure-salt
access and the generated trip wrapper. Thirty-seven quotes followed by a
password fit the escaped display bound when suppression omits the wrapper;
43 quotes still exceed it. Raw names above 100 UTF-8 bytes remain rejected.

PHP extraction requires UTF-8 mbstring and `default_charset`, substitution
character 63, and the previously recorded source hashes and worker bounds.
The exhaustive CP932 table and 48 encoding/hash vectors remain unchanged.

## Database and admission

Migration 0068 adds the operator-controlled `strip_tripcode` boolean with a
false default and imports the 82 source values. It replaces the existing
invoker insert trigger in place. The trigger reads forced anonymity and trip
suppression under `FOR SHARE`, clears new trips when either applies, and
replaces an empty suppressed name with `Anonymous`. This also covers direct
approved-attachment SQL and binary rollback. Apply the migration before the
new binary and retain it when rolling back.

Historical rows, trip strings, clocks, unrelated board settings, trigger
identity/owner/security/search path and existing ACLs are retained. The
attachment function owner gains only `SELECT` on the new board column. Public
policy writes, saved-trip updates and direct trigger execution remain denied.
Turning suppression off restores trip generation for later posts.

The source leaves its private second field in `$trip` when suppression skips
hashing, and its separate trip spam hook can inspect that raw value. The
rewrite sends only a derived public legacy hash to admission; suppression
sends an empty value. Private password suffixes do not enter this hook or its
logs. This is an explicit backend security difference, not matching of that
unsafe source behavior.

## Verification

The populated upgrade passes all 82 defaults, retained identities and clocks,
future defaults, normal/secure/trip-only direct inserts, toggles, column grants
and actual runtime denials. A fresh private role bootstrap applies all 59
migrations through 0068 as the owner and preserves historical content and restricted
roles. The private cluster is removed afterward.

A fresh SQLx database also passes migration and seeding, with 59 successful
ledger entries and maximum version 68, the exact 82 source definitions, both
suppression overrides and the seeded demo content. Migration identifiers have
gaps; the initial local ledger assertion incorrectly treated version 68 as a
count. The corrected assertion requires the actual file count and final version.

The generators write canonical UTF-8 bytes with LF newlines, and their byte
checks pass on Windows and Linux. Final review found that Windows text output
had produced CRLF bytes for the unpublished migration. The earlier local
database and its recorded checksum were preserved unchanged. A separate fresh
database was migrated and seeded from the canonical files, and its recorded
0068 checksum matches those files. No applied migration ledger was rewritten,
and the published board-import migrations retain their committed bytes.

The focused HTTP case witnesses an actual PostgreSQL board-row lock wait,
changes the policy while the request is waiting, then requires a suppressed
secure reply without a key. Source-selected names are checked in storage,
escaped previews and both JSON listeners. The same thread retains its old
normal trip. Name rejection leaves reply counts and clocks unchanged; turning
suppression off restores normal trips and the missing-secure-key error.

Real approved-attachment checks pass for comment-plus-file and file-only
posting. Each saves suppressed normal, secure and trip-only identities through
the ordinary writer. A direct scoped insert supplies an empty name and a
transaction trip; the trigger must save `Anonymous` with no trip and retain
the approved attachment. Existing one-use and role-denial checks still pass.
All 139 native Windows domain tests pass with no failures or ignored tests.
Strict workspace Clippy for all targets/features, all three exact extraction
checks and PHP/Python/shell/JavaScript syntax checks pass. Seven focused browser
cases pass, including both source boards at desktop/mobile widths, native
script-free submission, live Quick Reply, exact JSON identity fields and the
existing CP932, preview, filter and mobile-header checks. The full Linux workspace
passes all 682 tests across 171 test groups with no failures or ignored tests.
All 39 affected public browser cases and the separate staff WebAuthn,
posting, moderation, recovery and logout case pass. The staff check requires
the exact source-encoded JSON name and decoded public rendering. Together,
these 40 cases cover identities, mobile headers, posting preferences, Quick
Reply, wordfilters, randomizers, anonymous ownership and staff workflows.
Complete hosted checks at this follow-up's pushed head remain required.

The first HTTP attempt used the public role to inspect all 82 definitions and
failed on the private board's RLS visibility. Inventory verification now uses
the owner; actual posting and response checks still use the public role.
No visibility grant changed. The first browser attempt waited for an input
submit control where the native form has a button. The trace retains that
locator wait. The corrected case uses the actual button with the same deadline.
A later local attempt exceeded the unchanged web-server startup allowance
during a cold binary build. The qualifier now prebuilds the binary, as the
hosted verification script already does; startup and test deadlines are unchanged.

The first separate staff browser attempt could not spawn its fixture helper
because the harness assumed `target/debug` while the local build uses
`CARGO_TARGET_DIR`. Both staff servers and child helpers now resolve that
configured directory, retaining the original default, role environments,
status assertions and deadlines. The missing-helper failure is retained locally.

The earlier `f2d1b62` hosted Linux run fails because a mobile-header assertion
expects a raw JSON name after source entity serialization was implemented.
It now requires the exact source entities and retains decoded rendered-name
assertions. The failed hosted log and browser artifacts are retained locally.
Windows visuals and media/operations pass at that head, as do monitoring and
advisory checks; its required aggregate gate fails. These results do not
qualify the later suppression change.

Reproduction commands, using the owned role environments:

```sh
python3 scripts/extract-trip-policy-reference.py 4chan-old fixtures/trip-policy-reference.json --migration migrations/0068_trip_suppression.sql --check
php scripts/extract-public-name-reference.php 4chan-old crates/domain/tests/fixtures/public-name.json --check
sudo bash scripts/test-trip-suppression-migration.sh
sudo bash scripts/test-role-bootstrap.sh
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p board-domain --locked
node node_modules/@playwright/test/cli.js test tests/browser/post-identities.spec.js tests/browser/mobile-post-headers.spec.js tests/browser/post-preferences.spec.js tests/browser/quick-reply.spec.js tests/browser/wordfilters.spec.js tests/browser/posting-randomizers.spec.js tests/browser/anonymous-session.spec.js
```
