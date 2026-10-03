# Source staff identity verification

This checkpoint restores source badge choices, public staff name preparation,
prepared trips and the administrator's forced-anonymous posting exception.
It extends [rank-specific posting limits](verification-authorized-post-limits.md).
Unbadged authorized posting, Pass/VIP behavior, staff attachments and other
privileged workflows remain unfinished. The full rewrite PR stays open.

## Independent source evidence

The extractors require revision `545b7812d1849f7958d914950c91fdbbe38f6b22`
and exact source-file hashes. They execute only bounded pure helpers or selected
predicates with synthetic values. They never load the application, private
configuration, authentication implementation, sessions or database code.

| Reference | Cases | What is compared |
|---|---:|---|
| `staff-capcodes.json` | 1,024 | Four ranks, global flags, allow/deny scopes, badge choices and name masking |
| `staff-name.json` | 553 | Seven board name policies, 255-byte authorized fields, whole-field cleanup, CP932 preparation and the finished name/trip bound |
| `staff-catalog-identity.json` | 64 | Both OP and last-reply predicates, forced anonymity, meta boards and saved badge values |
| `public-name.json` | 469 | Earlier ordinary-name results remain unchanged |
| `admission-normalization.json` | 1,862 | Earlier 1,849 results remain unchanged; thirteen native Unicode cases add ten explicit failures and three controls |

Source global flag checks require `all` or the literal empty board, with no
`noboard` deny. A named board allow alone does not grant global flag access.
The developer flag independently permits its badge. The administrator's name
exception depends on rank during posting. The catalog separately permits
names and trips only for saved `admin` or `admin_highlight` badges when forced
anonymity or meta-board policy applies. Both catalog layouts use that same
predicate for hover details and identity filters.

Secure trip hashing remains the documented local-key replacement. Only its
prepared display hash enters posts, proofs or public responses. The staff
application reads a separate `STAFF_TRIPCODE_KEY` variable; unrelated runtimes
and media jobs reject it. Operation and configuration are described in
[staff posting](staff-capcodes.md) and [posting identities](post-identities.md).

## Database and application boundary

Migration 0071 adds source choice, prepared trip and name-permission fields to
single-use proofs. Its SQLx SHA-384 checksum is
`280116e4992f01cb8fe26116a4e82be0c0a9ae1f3b50c194552624f024e038dba76ec9aa68f105b9921f83eedb48a61f`.
The owned test database records 62 successful migrations through version 71;
the ledger checksum matches the current file. Already-applied migration bytes
and earlier development ledgers are retained.

Fresh bootstrap checks the owners, column grants and runtime denials. The
populated upgrade starts with historical content, account flags and scopes,
and pending proofs from both earlier issuers. It checks unchanged rows,
function OIDs, owners and grants. Eligible old proofs still consume without
trip authority; missing source permissions reject without consuming the
proof. A new trip-only proof binds its prepared hash. The disposable upgrade
database is removed after the checks.

The posting suite compares 79 eligible source cases through actual HTTP,
persistence and both public listeners, including ten rejected size boundaries.
It checks stored names and hashes, thread HTML, updater/preview fragments and
JSON exclusion of private input. Additional cases cover all six badge choices,
global versus named-board flags, masking after finished-name validation,
administrator posting exceptions, board trip suppression and both catalog
layouts. Actual board-lock waits change badge or name flags before the request
continues; rejected requests leave post, proof and audit state unchanged.

Runtime SQL checks compare all 1,024 badge/name cases, deny direct execution of
the private helpers, bind every prepared proof field and reject wrong hashes,
replay, stale flags and forged transaction settings. Public callers cannot
obtain an administrator identity by setting staff-related GUCs.

## Qualification

All tests use synthetic records and owned localhost roles. The final local
checks include the corrected catalog predicate and deterministic fixture
inspection. Hosted checks are still required at the published PR head.

| Check | Result |
|---|---|
| Source extractor regeneration checks | Passed |
| Fresh role bootstrap | Passed; all 62 migrations and restricted grants |
| Populated migration 0071 upgrade | Passed; history, grants and eligible pending proofs preserved |
| Staff HTTP/PostgreSQL posting suite | 17 passed, including both corrected catalog layouts |
| Full Linux Rust workspace | 701 passed across 172 groups; no failures or ignored tests |
| Strict workspace Clippy | Passed with all targets/features and warnings denied |
| Native Windows domain suite | 146 passed across 18 groups; no failures or ignored tests |
| Client unit/DOM regressions | 109 passed; no failures, skipped or cancelled cases |
| Actual staff browser flow | Passed; synthetic WebAuthn, source badge selection, secure trip, updater, quote previews and forced-anonymous catalog identities |
| Public browser regressions | 141 passed, including catalog previews/teasers, text catalogs, updater, quote preview, filtering and search |
| Hosted checks at `0b0413c49b2f29944d2c54f476ecb5a8b58750ac` | Build/test failed; Linux wordfilter readiness race. Windows visuals and media/operations passed. Monitoring and advisories passed. |

The full-suite Unicode failure is preserved in the original Linux and Windows
logs and the checked-in property seed. The supplied PHP/ICU helper reports
`U_INVALID_CHAR_FOUND`, then coerces its failed result to empty matching text.
Rust continues to reject admission and now distinguishes `InvalidOutput`.
Every generated input is checked; there are no discarded cases, retries or
expanded bounds. Database regressions require no post, thread, hit, log or ban
to survive the native failure, followed by a successful control.

Reproduction commands, with the owned runtime roles and pinned native ICU
configured:

```text
python3 scripts/extract-staff-capcode-reference.py SOURCE apps/staff/tests/fixtures/staff-capcodes.json --check
php scripts/extract-public-name-reference.php SOURCE crates/domain/tests/fixtures/staff-name.json --authorized --check
python3 scripts/extract-staff-catalog-identity-reference.py SOURCE crates/domain/tests/fixtures/staff-catalog-identity.json --check
php scripts/extract-admission-normalization-reference.php SOURCE crates/domain/tests/fixtures/admission-normalization.json --check
bash scripts/test-role-bootstrap.sh
bash scripts/test-staff-identity-migration.sh
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --examples --bins --locked
npm run test:staff
```

Browser retries, startup/test deadlines, snapshot baselines and CI guards remain
unchanged. The separate staff origin, WebAuthn sessions, scoped database roles
and secure-trip replacement remain the documented security differences.

The expanded staff flow reached its final state check with the correct three
thread states in a different row order. Its synthetic inspector lacked an
`ORDER BY` because the earlier fixture had one thread. Ordering the inspector by
thread ID makes that comparison deterministic; the complete expected states
and audit sequence are retained. The first failure log and error context are
kept separately from the rerun evidence.

The hosted [build/test run](https://github.com/frankischilling/26chan/actions/runs/37099195172)
failed in the mobile `/v/` wordfilter browser case. Its trace shows the reply
link was clicked before the Quick Reply module finished loading. The ordinary
navigation completed, then the test waited for a Quick Reply textarea that
had not opened. The follow-up asserts that the native Quick Reply control is
attached before clicking the post-number link. All ten local wordfilter cases
pass with that assertion and the existing timeout, with no retries. This
subsequent fix does not qualify the unchanged `0b0413c` commit. The retained
hosted trace and error context establish the failure rather than a blind rerun.

The same head passed [monitoring](https://github.com/frankischilling/26chan/actions/runs/37099195158)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/37099195170).
The [source staff JSON follow-up](source-staff-json.md) adds the separate
meta-board setting and read projections. Its local and hosted results require
their own published-head qualification.
