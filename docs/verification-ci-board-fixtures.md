# Qualification after importing the original boards

Issue [#228](https://github.com/frankischilling/26chan/issues/228) tracks CI
fixtures that still assume an empty board inventory or earlier synthetic board
settings. Importing the pinned source definitions changes those assumptions.

The attachment restore fixture now checks the migration-installed inventory,
adds its own board, and verifies both after restoration. The fresh bootstrap
exercise separately checks the narrowly allowed staff credential identifiers
and executes denied credential reads and writes. These corrections preserve
the imported boards and existing authorization checks.

[Run 36911227243](https://github.com/frankischilling/26chan/actions/runs/36911227243)
on `536b2cd` passed media/operations, attachment restoration, dependency
advisories and monitoring. Linux passed the embed repair and later reached
the broad browser group, where 57 cases passed and the Unicode-limit case
failed before posting. It expected the former 4,000-character setting, while
the imported `/test/` configuration and public API report 2,000.

That test now checks the advertised limit against the independent pinned
`fixtures/board-reference.json` definition. It still posts supplementary
Unicode characters exactly at the boundary, rejects one extra character,
checks unchanged JSON and ETags after rejection, checks cleanup and emoticon
admission, and verifies multiline multipart CRLF and saved soft breaks.
JavaScript remains disabled for the ordinary-form scenario.

The same hosted run failed one of 1,173 Windows theme cases. The failure
diagnostics record `/static/thread-watcher.v1.js` failing with
`net::ERR_NO_BUFFER_SPACE`; other recorded scripts and stylesheets returned
200, and no page error was recorded. The saved screenshot shows the catalog
with its ordinary Settings links, but the native settings marker required by
the interaction test is absent.

At that failure, the host reported 918 TIME_WAIT connections, 16 Bound
connections, approximately 12 GiB of free memory and 239,534,080 bytes of
nonpaged pool. These measurements do not establish the cause. Microsoft's
[port-exhaustion diagnostic guidance](https://learn.microsoft.com/en-us/troubleshoot/windows-client/networking/tcp-ip-port-exhaustion-troubleshooting)
also cautions that TIME_WAIT counts alone do not prove exhaustion.

The existing bounded Windows diagnostic now also records the configured TCP
dynamic ranges, counts of observed local ports within each range, aggregate
handles/private memory and TCP states for the fixed test-process names, and
counts of recent TCP/IP exhaustion events when available. It emits no
endpoints, PIDs, command lines, event text, cookies or form data. The observed
port count is a diagnostic sample, not a count of all available ports. Failed
queries return null. The diagnostics do not change host network settings.

The first local diagnostic completed in under five seconds, emitted under
two KiB of aggregate JSON, and reported 16,384 configured TCP ports per address
family. The event query was unavailable and is recorded as null. Retries, exact assertions, screenshot
tolerance, baselines and test selection are unchanged. Issues #139 and #142
remain open until the intermittent failure's cause is established.

On the corrected integrated checkout, the full Linux group that failed in CI
passed all 58 scenarios without retries. This includes the Unicode boundary
scenario and the existing archive, catalog, search, worker, watcher-settings
and position cases. Command:

```text
node node_modules/@playwright/test/cli.js test tests/browser/behavior.spec.js tests/browser/archive.spec.js tests/browser/catalog-preferences.spec.js tests/browser/catalog-inplace.spec.js tests/browser/catalog-search-contract.spec.js tests/browser/catalog-live-search.spec.js tests/browser/catalog-thread-state.spec.js tests/browser/catalog-search-fields.spec.js tests/browser/native-filter-worker.spec.js tests/browser/catalog-filter-worker.spec.js tests/browser/watcher-settings.spec.js tests/browser/watcher-position.spec.js
```

The unchanged Windows settings lifecycle file passed all 22 cases. Twenty
additional repetitions of the specific failed interaction also passed, each
with a fresh worker and no retries. These focused local passes do not reproduce
or explain the hosted failure. Both resource samples emitted valid bounded
JSON; the event query remained unavailable. The full 1,173-case hosted theme
group remains a qualification requirement.

```text
node node_modules/@playwright/test/cli.js test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js
node node_modules/@playwright/test/cli.js test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js --repeat-each 20 --grep "a competing catalog-theme edit cancels a queued save"
```

The hosted run failed overall, and later Linux browser groups did not run.
The final current-head checks must pass before #228 can close or the complete
rewrite can merge.
