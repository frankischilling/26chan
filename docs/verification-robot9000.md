# Robot9000 verification

This checkpoint implements public Robot9000 posting and capcoded staff bypass
for #211 on the supplied source revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. Anonymous staff bypass and the
administrative cleanup interface remain in #222. The complete rewrite stays
tracked in #191. [Behavior and deployment notes](source-robot9000.md) describe
the active source branches and security adaptations.

Checks run on October 1, 2026:

| Command | Result |
| --- | --- |
| `cargo test -p board-domain robot9000 --locked` | 8 passed, including extracted normalization/duration cases, canonical board-specific actor keys and 128 bounded generated-input cases |
| `php scripts/extract-robot9000-reference.php 4chan-old fixtures/robot9000-reference.json --check` | 25 reduction cases and 11 durations matched in WSL; the source directory was supplied by its absolute path from the worktree |
| `cargo test -p board-store --test robot9000 --features database-tests --locked` | 3 passed on Windows against separate disposable PostgreSQL 16 |
| `cargo test -p board-public --test robot9000 --lib --all-features --locked` | 101 Windows library tests and the real HTML/JSON posting test passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed under Ubuntu/WSL before the final HTTP and attachment assertions were added |
| `cargo test -p board-staff --test posting authenticated_capcoded_staff_bypass_robot9000 --features database-tests --locked` | Passed in Ubuntu/WSL; revoked staff remained denied |
| `bash scripts/test-role-bootstrap.sh` | Fresh migrations, two-way historical comparisons, exact function-owner/runtime grants and actual negative reads/updates passed; private Unix-socket cluster removed |
| `npx playwright test tests/browser/robot9000.spec.js` | Passed without retries on Windows Chromium, covering desktop and a touch-enabled mobile context |

Persisted checks exercise disabled-board behavior, unauthenticated bypass
options, private board denial, missing identity, normalization collisions,
simultaneous duplicate submissions, active mutes, progressive and capped
durations, single-level decay, low-content percentages and retained history
after deletion. Rejected replies leave thread counts, bump and change clocks
unchanged. Invalid parents/capabilities cannot change robot state. A
rejected OP at the thread limit retains the original visible thread. An
approved upload survives duplicate and textless rejection and attaches once to
a later valid reply. History and mute capacity exhaustion reject the whole
posting transaction. Direct table access, role assumption and policy writes
are denied under actual public credentials.

The real public handlers distinguish socket actors even with identical spoofed
forwarded headers. HTML errors retain escaping and no-store responses; JSON
contains only the source error. Capacity failures retain 503. Public post JSON
contains neither addresses nor private state fields. The browser test uses real
ordinary forms and Quick Reply, checks unchanged failed drafts, accepted reply
insertion and a rejected duplicate without creating another post.

The PHP extractor reads only the pure reduction block, constants and duration
formatter on synthetic inputs. It never executes the plugin's database-backed
process function or loads private configuration. Its fixture records the plugin
and posting-file hashes. The source image/secret-command branches are inactive.

Failed attempts are retained as failures. The first worktree migration run found
line-ending differences in two historical migration files; their normalized
contents matched the main checkout, and their exact existing bytes were restored
for the owned test database. A second attempt found the new draft's number
collided with existing migration 0061. The new migration is now 0062. Tests run
before it was applied failed on missing columns. A fresh, separate database
then applied the final migration and seeded successfully; no stored migration
checksum was edited.

The first private-state test inferred integer arrays for synthetic bytea input;
explicit byte literals corrected the test. The new attachment fixture initially
left its board's existing zero-image limit enabled, and its first cleanup missed
the attachment foreign key. Both fixture setup and cleanup were corrected, and
the affected owned fixture was removed. The first HTML assertion expected named
entities instead of Askama's numeric escaping, and an API assertion accidentally
matched the public subject's semantic URL; the corrected checks require escaping
and absence of private fields. The first browser attempt tried to fill a hidden
mobile ordinary form while Quick Reply was enabled. It now uses the real disabled
Quick Reply preference to open that form, then enables Quick Reply for its own
checks. The complete relevant commands passed afterward.

An initial all-workspace Windows check could not build vendored OpenSSL because
Perl was absent from PATH. It is not counted as a pass; the WSL workspace check
and real staff test used the installed toolchain. Current-head CI and full
integration qualification must still run after this work is committed.
