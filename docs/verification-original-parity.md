# Original board and comment-link verification

This checkpoint covers the original board inventory, supported board policies,
server comment links and catalog teaser projection. The full rewrite remains
tracked in #191. These results do not establish complete frontend, media or
staff parity.

The reference is the supplied `4chan-old` checkout at revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The two synthetic fixtures record
source-file hashes. The formatting extractor executes only selected pure
functions on synthetic text, with PHP 8.3.6 and UTF-8 mbstring. It does not load
the old application, configuration, credentials or database.

On October 1, 2026, the following checks passed locally:

| Check | Result |
| --- | --- |
| `python scripts/extract-board-reference.py 4chan-old fixtures/board-reference.json --migration migrations/0045_original_boards.sql --check` | All 80 listed boards and both configuration-only boards match |
| `php scripts/extract-format-reference.php 4chan-old fixtures/format-reference.json --check` | 35 link cases and 52 teaser cases match |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed in Ubuntu under WSL |
| `cargo test --workspace --all-features --locked` | 556 passed, none failed or ignored, with disposable PostgreSQL credentials |
| `npm run check:generated` | All five generated browser bundles match their sources and fit existing budgets |
| `node --test tests/browser/native-updater.test.mjs tests/browser/native-quote-preview.test.mjs tests/browser/native-linkification.test.mjs` | 36 passed, none failed or skipped |
| `npx playwright test tests/browser/static-quotes.spec.js tests/browser/native-linkification.spec.js tests/browser/catalog-teasers.spec.js tests/browser/word-breaks.spec.js` | 12 passed on Windows Chromium |
| `sudo bash scripts/test-static-quote-migration.sh` | Disposable upgrade, historical profiles, new stamping and restricted insertion passed |

The Rust run used pinned Rust 1.94.0. Linux browser-dependent tests used Node
24.14.0; the focused Windows browser run used Node 25.2.1. CI pins Node 24.14.0
and runs the full verification script. Local results do not substitute for the
checks on the pushed revision.

The inventory integration test checks every supported field and available
public HTML/JSON route. It creates healthy private board, thread, post and
deletion rows using the owner connection, then verifies that public credentials
cannot read those rows or insert private content. Directory links retain source
order, `/j/` is omitted and `/f/` does not expose an ordinary catalog. The
private staff interface remains part of #221.

Posting tests cover both accepted form encodings, both posting endpoints and
HTML/JSON responses. They verify committed comments, saved profiles, local
destinations, leading-zero labels, JSON updater HTML, catalog metadata and
historical rendering. Staff previews use the same parser with inert quote
navigation. Browser tests cover script-free rendering, optional external URL
decoration and actual live reply insertion.

The browser run found a real updater defect: its safe URL parser rejected new
board, catalog and rules references. The fix admits those finite local routes
and retains tests against unsupported paths and unsafe encodings. The board
and archive templates also needed consistent LF line endings for their exact
HTML checks. Neither correction changed resource budgets or removed assertions.

One earlier Quick Reply check reported a connection error despite a successful
POST and valid updater response while the full Rust run was also active. Its
cause has not been established. The complete focused browser command later
passed in isolation, without retries or relaxed assertions. Windows interaction
stability remains tracked in #142.

Desktop and mobile post/card screenshots were inspected for text wrapping,
literal disabled spoiler tags, escaped markup and external URL decoration.
They are component evidence. Full original-page comparison remains in #193.
This checkpoint makes no new claim about production media qualification,
specialized board features or complete staff authorization.
