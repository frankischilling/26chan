# Provider embeds after the source formatter change

The supplied `4chan-old` revision
`545b7812d1849f7958d914950c91fdbbe38f6b22` keeps external provider URLs as
text until the optional native linker runs. `Media.replaceYouTube` and
`Media.replaceSoundCloud` in `js/extension.js` add controls to plain text when
`Config.linkify` is false. This repair covers that branch and the HTML entity
spelling used by native `/derefer` links.

[CI run 36903933543](https://github.com/frankischilling/26chan/actions/runs/36903933543)
on `f825d6064b3b4d848a3d71587afbcf51cd37a7ce` passed media/operations,
attachment restoration and Windows visuals. Dependency advisories and separate
monitoring checks passed too. The Linux application job passed 584 all-feature
workspace Rust tests, its preceding 104-test public library and HTTPS proxy
check, Global Search and dice/fortune browser scenarios, then failed both embed
scenarios. Subsequent browser steps did not run, and the aggregate job failed.

The failure reproduced on Windows. Desktop controls were absent because the
controller only scanned anchors. Mobile controls were absent because the
redirect parameter held `&amp;` while the visible URL held `&`. The controller
now scans bounded text runs, retains soft breaks in source spans and requires
native redirect destinations to match their visible label after one entity
decoding pass. Existing provider URL validation and click-only loading still
apply. Disabling the feature unwraps its source spans and removes its controls
and players.

Checks run on October 1, 2026:

| Command | Result |
| --- | --- |
| `npm run test:embeds-core` | 10 passed on Windows; includes raw text/soft breaks, unchanged projection text, native entity decoding, atomic character/HTML-limit rejection and no request before clicking |
| `node --test tests/browser/native-quote-preview.test.mjs tests/browser/native-quote-preview-dom.test.mjs tests/browser/native-linkification.test.mjs tests/browser/native-linkification-dom.test.mjs` | 55 passed on Windows |
| `npx playwright test tests/browser/native-embeds.spec.js tests/browser/native-linkification.spec.js tests/browser/posting-randomizers.spec.js` | 12 passed on Windows Chromium, without retries |
| `cargo test -p board-public --test ui_assets --features database-tests --locked` | 9 passed on Windows against disposable PostgreSQL 16; exact asset, size, CSP and listener boundaries remain checked |
| `node --test tests/browser/native-embeds.test.mjs` followed by `node node_modules/@playwright/test/cli.js test tests/browser/native-embeds.spec.js` | 10 controller tests and all 3 real-thread scenarios passed in Ubuntu under WSL; the final HTML-limit assertion was then added and passed on Windows |

The real-thread browser scenarios cover plain provider text, encoded native
links, desktop Embed controls, mobile Open controls and quote previews that
retain source URLs while omitting controls and players. Provider requests are
observed and aborted in the owned browser context. The tests do not contact
third-party services to qualify their availability.

An earlier combined browser run rejected repeated preview-spacing text under
the existing spam rule. The fixture now uses distinct numbered lines; the
complete 12-scenario command passed afterward. The first local Linux rerun
aborted before browser tests because a concurrent Windows test server occupied
port 3000. That aborted attempt is not a pass; server reuse stays disabled.
The isolated rerun built the Linux public binary and passed all three browser
scenarios, without retries.

The complete rewrite remains tracked in #191. A later passing CI run must
qualify the repair and execute the steps skipped by the failed run.
