# Dice and fortune verification

This checkpoint implements #210 against the supplied `4chan-old` revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. It also corrects the role-bootstrap
assertion that failed CI after staff session locking was added. The full rewrite
and final CI qualification remain tracked in #191 and #228.

The source extractor executes the isolated dice block and pure catalog
truncation function on bounded synthetic inputs. It reads the active fortune
list without loading the old application, private configuration or database.
The checked fixture records both source-file hashes. The seasonal fortune list
is commented out in the supplied source and is not treated as active behavior.

Source-spelling correction, October 7, 2026:

The isolated PHP oracle now covers 31 dice cases and eight explicit resource-bound
rejections, alongside the 13 fortunes and eight teasers. It records leading-zero
counts and sides, the 25-roll cap, modifier spelling and incomplete patterns.
The expanded stored-result test checks repeated reads of both padded and
ordinary labels. Browser scenarios carry padded labels through reload, JSON,
catalog, search, RSS, updater and quote previews. Local qualification of these
changes passed the full 194-test domain suite, the persisted randomizer test,
128 public-library cases and both server-link/word-break HTTP regressions. One
public Unix-socket test remains excluded locally after socket creation was denied.
Strict workspace Clippy passed. The two browser cases were discovered but not
executed locally; hosted qualification remains pending. The earlier results
below apply to their recorded versions.

Checks run on October 1, 2026:

| Command | Result |
| --- | --- |
| `php scripts/extract-randomizer-reference.php 4chan-old fixtures/randomizer-reference.json --check` | 9 deterministic dice cases, all 13 fortunes/colors and 8 catalog teaser cases match the source |
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p board-domain posting_randomizers --locked` | 7 passed, including 128 bounded generated-input cases |
| `cargo test -p board-public --lib generated_randomizers_follow_source --locked` | Source catalog teaser comparison passed |
| `cargo test -p board-public --lib --locked` | 100 passed on Windows after correcting the new RSS test's leading-whitespace assumption |
| `cargo test -p board-store --test posting_randomizers --test search --features database-tests --locked` | Both persisted tests passed on Windows against disposable PostgreSQL 16 |
| `npm run check:generated` | All six bundles match their sources and existing size limits |
| `node --test tests/browser/native-updater.test.mjs tests/browser/native-quote-preview.test.mjs tests/browser/native-quote-preview-dom.test.mjs tests/browser/native-backlinks.test.mjs tests/browser/native-backlinks-dom.test.mjs tests/browser/native-linkification.test.mjs tests/browser/native-linkification-dom.test.mjs tests/browser/global-search.test.mjs` | 103 passed, none failed or skipped |
| `npx playwright test tests/browser/posting-randomizers.spec.js tests/browser/global-search.spec.js` | 6 passed on Windows Chromium before the additional updater/RSS/preview assertions |
| `npx playwright test tests/browser/posting-randomizers.spec.js` | Expanded desktop and real mobile-device scenarios both passed, without retries |
| `sudo bash scripts/test-role-bootstrap.sh` | Fresh migration, historical preservation and restricted role checks passed in an isolated Unix-socket cluster; cleanup completed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed in Ubuntu under WSL, including the rerun after the final RSS test was corrected |
| `cargo test --workspace --all-features --locked` | Recovered integration run passed 583 tests, none failed or ignored; its public binary preceded the final RSS test |
| `cargo test -p board-public --lib --all-features --locked` | Final public library passed all 104 tests in WSL, including the new RSS escaping case |

The browser scenarios create and delete their own threads. They check fixed
one-sided dice through page reload, JSON, catalogs, search, RSS and a reply
inserted by the real updater. They check a retained fortune, its computed color,
remote quote previews, disabled-board behavior and absence of script or content
policy errors. The 390-pixel scenario uses a mobile user agent and real touch
input. Desktop and mobile fortune screenshots were inspected for text, color,
wrapping and page layout; complete original-page visual comparison remains in
#193.

The fresh bootstrap comparison ignores only newly added historical columns and
separately requires them to remain NULL. All older content and thread fields
still compare in both directions. The staff posting owner must have credential
identifier reads for session locking. The check permits exactly `id` and
`account_id`, forbids passkey-document reads and all credential writes, and
executes denied reads, identity updates and deletes under the restricted role.

The [CI run on the preceding search checkpoint](https://github.com/frankischilling/26chan/actions/runs/36898413787)
passed Windows visual qualification but failed Linux setup because the old
bootstrap assertion forbade those required identifier reads. Dependency
advisories and separate monitoring qualification also passed on that checkpoint.
Those results do not qualify the later dice/fortune changes or make the failed
aggregate check pass.

[Run 36903933543](https://github.com/frankischilling/26chan/actions/runs/36903933543)
on `f825d6064b3b4d848a3d71587afbcf51cd37a7ce` subsequently passed all 584
all-feature workspace tests, the preceding public library and HTTPS check,
all four search browser scenarios and both expanded dice/fortune scenarios.
Media/operations, attachment restoration and Windows visual qualification
passed. The Linux job then failed the native embed scenarios after external
URLs became plain source text. The [embed verification record](verification-embed-source.md)
describes that repair. The run's aggregate result is failed; subsequent browser
steps were not executed.

A first browser invocation timed out while rebuilding the server; it was not
counted as a pass. The first public-library run passed 99 tests and failed the
new RSS test because it assumed the existing template had no leading whitespace.
The assertion now trims that whitespace before checking the generated prefix;
the complete 100-test library command passed afterward. A WSL restart
interrupted an earlier broad workspace test run and
removed its disposable database. The subsequent Windows persisted-test attempt
failed to connect to that stopped database. A new owned cluster was migrated
and seeded, and both focused persisted tests passed. The interrupted full run
is not recorded as successful. The recovered integration run completed with
583 passing tests, including the actual HTTPS proxy, browser attachment,
private staff discussion and persisted store tests. After the final RSS test
was added, workspace Clippy and the 104-test all-feature public library passed
again. Final pushed-head CI qualification is still separate.

For local reproduction, use the disposable setup in `README.md`,
apply all migrations and seed `fixtures/demo.sql`. Rust database tests need the
owner and separate runtime test credentials; Playwright's public server strips
privileged credentials from its environment. Migration 0055 is additive and
must precede deployment of the new binary. Keep its columns for binary rollback.
This checkpoint makes no new claim about worker containment or production
deployment; the existing media qualification and operational evidence remain
separate requirements.
