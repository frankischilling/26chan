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
JSON; the event query remained unavailable. These local checks preceded the
full hosted theme run described below.

```text
node node_modules/@playwright/test/cli.js test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js
node node_modules/@playwright/test/cli.js test --config playwright.themes.config.js tests/themes/catalog-settings-lifecycle.spec.js --repeat-each 20 --grep "a competing catalog-theme edit cancels a queued save"
```

The hosted run failed overall, and later Linux browser groups did not run.
The final current-head checks must pass before #228 can close or the complete
rewrite can merge.

[Run 36919534871](https://github.com/frankischilling/26chan/actions/runs/36919534871),
on `68c5a48`, passed the full Windows visual job, including all 1,173 theme
cases. Media/operations, restoration, dependency advisories and monitoring
also passed. This Windows pass does not explain the earlier intermittent
resource failure, so #139 and #142 remain open.

Linux passed all 58 scenarios in the corrected behavior group and the later
groups through the eight updater scenarios. Its notification file then passed
five cases and failed the two filter-generation cases. They still expected
the server to turn `https://lower.test/path` into an anchor. The pinned source
server only links its own domains; external URLs remain text for the optional
browser linker. The saved response correctly contained both external URLs as
text, so the assertions failed before either updater completion check ran.
The aggregate workflow failed and subsequent browser groups did not run.

The notification fixture now posts a source-domain URL alongside the lowercase
and uppercase external URLs. It requires exactly the source-domain server
anchor, neither external server anchor, and no browser decoration in JSON.
Both preference settings retain the exact update-event highlight, notice and
completion checks. Enabling linkification must produce exactly two browser
anchors with the expected text and encoded redirect destinations while
preserving the source-domain anchor. Disabling it must produce none.

All seven unchanged and corrected notification scenarios passed against the
integrated public process and disposable PostgreSQL database in Linux without
retries:

```text
node node_modules/@playwright/test/cli.js test tests/browser/native-updater-notifications.spec.js
```

The integrated checkpoint `3b380888c5455292730a9fa568c14475d9b89d09` passed
[run 36931496972](https://github.com/frankischilling/26chan/actions/runs/36931496972)
in full: Linux Rust/browser checks, the Windows visual job with all 1,173 theme
cases, media/operations and the required aggregate gate.
[Dependency advisories](https://github.com/frankischilling/26chan/actions/runs/36931496969)
and [monitoring](https://github.com/frankischilling/26chan/actions/runs/36931496979)
also passed on that checkpoint.

Local checks passed 22 subsequent browser cases. Six tail cases stopped in
setup because their owned fixture executable was unavailable; none reached
the tail assertions. After building the existing helper, all six passed
without assertion, retry or timeout changes.

These results qualify that integrated checkpoint. New feature work needs its
own current-head checks. Issue #228 remains open through complete rewrite
qualification and merge; the intermittent Windows cause in #139/#142 remains
unresolved.

The wordfilter checkpoint `9ad321e` passed its Windows visual job, including
all 1,173 theme cases. Its media/operations job passed the media qualification
and migration checks before failing at restore-fixture setup. That fixture
inserted raw historical comments on the original `/test/` board, whose imported
filter policy now requires a retained result. It now uses the separate
unfiltered `/fixture/` board. The filter guard and restore assertions are
unchanged. A local rerun against a fresh PostgreSQL 16.15 cluster passes the
complete restore exercise. The selected test port is validated, and the script
still verifies that it belongs to its generated disposable cluster. The next
pushed head requires its own full CI result.

On `e669a41`, [run 36962497073](https://github.com/frankischilling/26chan/actions/runs/36962497073)
passed media/operations, including restoration, and the complete Windows
visual job. Linux was still passing successive browser groups when GitHub
cancelled it at the job's 40-minute limit. The check annotation explicitly
reports that execution limit. The aggregate gate consequently failed, so
this head is unqualified. The Linux job now permits 60 minutes for the full
suite; test assertions, per-test limits, retries and required jobs are
unchanged. The corrected head requires its own completed CI run.

The corrected checkpoint `3b6300a` passed
[run 36966578661](https://github.com/frankischilling/26chan/actions/runs/36966578661)
in full: Linux Rust/browser checks, Windows visual checks, media/operations
and the aggregate gate. Its
[dependency advisory run](https://github.com/frankischilling/26chan/actions/runs/36966578669)
and [monitoring run](https://github.com/frankischilling/26chan/actions/runs/36966578657)
also passed. Subsequent feature changes still require their own qualification.

The October 2 client repeat exposed a separate BFCache test assumption in
`native-layout-dom.test.mjs`. A settings save and immediate teardown used two
Playwright `evaluate` calls. A valid pending theme notification could settle
between those browser tasks, producing `tomorrow,photon,tomorrow` where the test
expected the pending notification to be cancelled. Both events now run within
one browser task so the fixture actually exercises cancellation. No product
code, event assertions, deadlines, retries or visual expectations changed.
All seven layout DOM cases and the complete 481-test client repeat pass.
The failed repeat remains in the local qualification record. This fixture fix
does not explain the earlier `ERR_NO_BUFFER_SPACE` or upload-body CI failures.

The next content-admission checkpoint, `8195d65`, exposed a Windows startup
fixture dependency in
[run 37022763521](https://github.com/frankischilling/26chan/actions/runs/37022763521).
Nine child-process checks failed with empty application error output because
their cleared environments omitted ICU's DLL directory. The same nine failures
reproduce locally. A child launched without the loader path exits with
`-1073741515` and no stderr; restoring that path reaches Rust's expected media
configuration error. Startup children now retain only Windows `PATH` and
`SystemRoot`, with application configuration and credentials supplied explicitly.
All ten Windows startup cases and both paired-listener cases pass locally.
The metrics case also requires its intended configuration/bind error, so an
unrelated startup failure cannot satisfy its socket assertions.

That hosted checkpoint passes media/operations, including the actual isolated
Quick Reply upload, restoration and maintenance outcomes. Dependency advisory
and monitoring checks also pass. Linux Rust/browser checks and the aggregate
gate have now passed; the run fails solely at Windows startup, preventing this
head from qualifying. A later passing upload does
not establish the cause of the earlier response-body failure. The corrected
head still needs every hosted check.

The corrected startup checkpoint `a6fe0f3` passes
[run 37028174415](https://github.com/frankischilling/26chan/actions/runs/37028174415)
in full, including Linux Rust/browser checks, all Windows visual groups,
media/operations and the aggregate gate. Its
[advisory](https://github.com/frankischilling/26chan/actions/runs/37028174884)
and [monitoring](https://github.com/frankischilling/26chan/actions/runs/37028174423)
runs also pass. Later changes still require complete checks at their own head.

The fixed name/trip follow-up encountered a local media lease assertion failure.
Its focused diagnostic passed the assertion, leaving the original cause
unconfirmed. The revised fixture witnesses an actual job-row lock wait before
shortening the lease and waits for PostgreSQL to observe expiry before releasing
the lock. Reservation and approval still require `Conflict`, an unchanged
processing job and no approved output. Separate negative controls replaced each
final wall-clock predicate with a transaction timestamp; both fail for accepting
the expired lease. The mutations were reverted and all four media approval cases
pass. Application media code and runtime role limits are unchanged.

The next checkpoint, `d44ea1f`, fails
[run 37036025365](https://github.com/frankischilling/26chan/actions/runs/37036025365)
at the Linux Robot9000 browser case. Rust, media/operations and all Windows
visual groups pass; the required aggregate gate fails. The retained browser
trace shows the mobile submit button staying hidden, with no mobile POST sent.
The comment was edited before the form script initialized. Initialization
required an unchanged quote draft to reveal the form and therefore hid it.

A controlled regression holds the actual client bundle request, edits the
visible fallback field, then releases the bundle. It fails on the same hidden
field before the fix. Mobile quote-form visibility now depends on the validated
quote URL rather than mutable draft text. The regression also requires the edit
to survive initialization and a real form submission to save it.
With the correction, all 15 Quick Reply, preference and Robot9000 browser cases
pass, including script-free desktop/mobile posting and invalid quote targets.
All 481 client tests and generated-bundle checks pass. Test deadlines, retries,
posting assertions and screenshot expectations are unchanged.

The same checkpoint's
[monitoring run 37036025377](https://github.com/frankischilling/26chan/actions/runs/37036025377)
fails before the SIGTERM cleanup exercise: choosing and immediately closing
one ephemeral socket at a time let the OS return a duplicate port. Qualifiers
now reserve their whole port set before releasing it. Core, authenticated,
queue, resource and maintenance callers use that batch API; receiver collision
checks remain. This prevents reuse within the selected set, without claiming
that reservations remain held while child processes bind their listeners.

Local Linux checks pass both 20-test monitoring suites, actual core and
authenticated scraping/alert delivery, queue transitions and unavailable-source
recovery, and each profile's OS SIGTERM cleanup. Windows runs 18 core tests with
two explicit Linux-only skips. Actual resource setup stops at its required
cgroup-v2 check because that interface is absent in this WSL instance;
maintenance requires the same interface. Both remain required hosted checks.
The checkpoint's
[dependency advisory run](https://github.com/frankischilling/26chan/actions/runs/37036025524)
passes. These corrections still require complete CI on their new head.

The corrected checkpoint `b4f0ef65eeff9eb91264b2a6596e5f398ab03277` passes
[build/test run 37040774446](https://github.com/frankischilling/26chan/actions/runs/37040774446)
in full: Linux Rust/browser checks, every Windows visual group,
media/operations and the required aggregate gate. Its
[monitoring run 37040774471](https://github.com/frankischilling/26chan/actions/runs/37040774471)
and [advisory run 37040774637](https://github.com/frankischilling/26chan/actions/runs/37040774637)
also pass. Hosted monitoring includes the actual resource and maintenance
exercises that require cgroup v2. These results qualify that checkpoint;
later feature work still requires checks at its own head.

The public-name checkpoint `f2d1b62a1cf6515985d711ef637a60e9f9a93609` fails
[build/test run 37054194081](https://github.com/frankischilling/26chan/actions/runs/37054194081).
Media/operations and every Windows visual group pass. Linux Rust checks pass,
but the mobile-header browser case still expects a raw name in JSON instead
of the newly matched source HTML entities. Its decoded rendered-name checks
pass. The required aggregate gate fails. The next follow-up corrects the exact
JSON expectation without changing the response contract or browser deadline.
The failed log and synthetic browser artifacts are retained locally.

That head's [monitoring run 37054194030](https://github.com/frankischilling/26chan/actions/runs/37054194030)
and [advisory run 37054194191](https://github.com/frankischilling/26chan/actions/runs/37054194191)
pass. This partial result does not qualify the later board trip-suppression
change; it still needs complete hosted checks at its own head.

The trip-suppression checkpoint `c9c054b5a3afc02b42e71eb05ec5f5c6e0484889`
fails [build/test run 37074316118](https://github.com/frankischilling/26chan/actions/runs/37074316118).
Media/operations and every Windows visual group pass. Linux Rust checks pass,
but a script-free field test still expects a 100-byte emoji name to survive
source cleanup. The saved and rendered result is `Anonymous`. The follow-up
retains the same raw emoji input, subject assertions and both 101-byte rejection
checks, and requires that exact source result in HTML and JSON. The failed
hosted log is retained locally. No browser deadline or screenshot tolerance
changes.

That head's [monitoring run 37074316097](https://github.com/frankischilling/26chan/actions/runs/37074316097)
and [advisory run 37074316143](https://github.com/frankischilling/26chan/actions/runs/37074316143)
pass. Its aggregate build gate fails, so these partial results do not qualify
the later authorized-limit work.
