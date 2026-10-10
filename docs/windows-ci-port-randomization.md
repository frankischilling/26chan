# Provisional Windows CI port-randomization trial

The ordinary Windows visual jobs now have a narrowly scoped trial that disables
Chromium's `TcpPortRandomizationWin` feature. It is intended to test whether the
required CI workloads become reliable under this launch configuration. One
successful diagnostic arm does not establish a fix, a Windows root cause or
lasting reliability. The full ordinary Windows gate and every other exact-head
check must pass before merge.

## Recorded matched observation

[Run 38080782241](https://github.com/frankischilling/26chan/actions/runs/38080782241)
tested head `e8f440dbe8a0788413110b1806ad7d742d1fc114`, attempt 1, with one
enabled/disabled pair of the unchanged full theme shard 7. Both arms executed
all 148 cases in their original order, with one worker at a time and zero
retries. The enabled arm passed 147 and failed one; its original exit was 1.
The disabled arm passed all 148 and exited 0. The pair checker qualified the
evidence as `matched-observation-only`; it did not turn the failed arm green.

The [enabled-arm artifact](https://github.com/frankischilling/26chan/actions/runs/38080782241/artifacts/11680288571)
records `/static/flags/flags.css` failing with `net::ERR_NO_BUFFER_SPACE`.
Its admitted NetLog links request 10073 to stream 10076, connection job 10077
and socket 10078. The connect attempt reports Windows error 10055 and Chromium
error -176. This connects the missing stylesheet to that socket failure;
it does not identify the resource that failed.

The [disabled-arm artifact](https://github.com/frankischilling/26chan/actions/runs/38080782241/artifacts/11680436978)
has complete command-line proof and a passing workload, but no retained NetLog:
the existing success path discards that capture. Absence of a retained log is
not a claim that every internal connection attempt succeeded. Normal failed
worker replacement produced two enabled-arm browser proofs versus one disabled
proof. Concurrency stayed at one and no test was retried; browser lifetimes
were consequently different.

The matched identity and content digests were:

- Runner image: `win25-vs2026`, version `20260925.250.1`.
- Windows release `10.0.26100`, full build `26100.33438`, x64, Node `v24.14.0`.
- Playwright `1.62.0`; Chromium headless shell `151.0.7922.34`, revision `1234`.
- Shared fixture: 25,904,640 bytes, SHA-256
  `c272a497ef55b458558293296a7856b7daccebb277104779efa13d7d2a5ba8bf`.
- Browser executable SHA-256:
  `ce4635cd0e5dc0e21494542a701f347e91c1f1d821970578d97ed8df4ced50ef`.
- Ordered 148-case digest:
  `c40fe2a250840b07735adaacdf8bd3879fbc064549a568973d9bc1a793ec8325`.
- Enabled-arm admitted NetLog: 8,340,969 bytes, SHA-256
  `0b5b222054e3a377401df21fba20531764f056c2beebf85ed14fbfc33c9ca488`.

Both diagnostic arms added `--enable-automation` for in-memory
`Browser.getBrowserCommandLine` verification, and repeatedly hashed their
executables. That instrumentation affects browser configuration and host load.
The ordinary-CI trial below adds neither automation nor CDP verification and
does not claim equivalence to the instrumented pair. The artifact links above
have short retention; this normalized summary preserves their result and
identity without copying raw command lines or private profile paths.

Chromium's
[pinned feature definition](https://github.com/chromium/chromium/blob/151.0.7922.34/net/base/features.cc#L257-L263)
enables this feature on supported Windows versions. Its
[connect implementation](https://github.com/chromium/chromium/blob/151.0.7922.34/net/socket/tcp_socket_win.cc#L1046-L1078)
ignores the return value of `setsockopt(SO_RANDOMIZE_PORT)`. Verified feature
configuration therefore does not prove that any particular option call
succeeded. The [native IPv6 reproduction](windows-socket-diagnostics.md#native-dual-stack-result)
already captured error 10055 with balanced userspace cleanup. Neither
experiment establishes that the native and browser failures share a cause.

## Exact scope of the trial

Only five steps in required `ci.yml` set
`WINDOWS_CI_DISABLE_TCP_PORT_RANDOMIZATION=1`:

1. Core visual fixture screenshots.
2. Archive desktop/mobile visual fixtures.
3. Attachment desktop/mobile visual fixtures.
4. Public empty/error desktop/mobile states.
5. The original theme execution step, across all eight unchanged shards.

The helper in `tests/helpers/windows-ci-port-randomization.js` requires that
exact opt-in, Windows and GitHub Actions CI. A malformed or out-of-scope opt-in
fails closed. With no opt-in, it returns the original launch-options object and
the public config has no added `launchOptions` property. Linux, local Windows
runs and production application behavior are unchanged.

The common integration is `playwright.public-base.js`, which all five target
suites inherit. This reaches both tests importing ordinary Playwright and tests
using the existing visual-diagnostics fixture. The latter still adds its
original bounded NetLog options. The other Windows core steps retain their
existing behavior: direct `chromium.launch` calls in Node tests bypass this
config; settings-categories has an independent config; custom-spoiler and
source-flag steps do not set the opt-in.

The separate browser-failure-control workflow remains on its existing baseline
in both the original browser and isolated sidecar. It is outside this ordinary
CI trial; its result must still pass before merge when triggered. Mixing a
mitigated original with an unmitigated sidecar would change that experiment.
Native socket diagnostics and their options remain unchanged. The deliberately
failed comparison workflow is not included in this trial branch.

## Default preservation and qualification

The helper checks all three Playwright package versions, both pinned Chromium
browser descriptors and the core bundle SHA-256:
`3258d1cf334c6afc95f22aa9c292436cb976b391e0437f1359c83b84f0cb9d66`.
Version, revision or default-source drift requires review. Existing caller
launch options and launch-affecting environment overrides are rejected. The
hosted image's inert `SELENIUM_JAR_PATH` marker and existing owned diagnostics
remain permitted.

Playwright filters `ignoreDefaultArgs` against its entire final argument list.
The helper removes only the exact original `--disable-features` switch and
replaces it with all 16 existing disabled features plus
`TcpPortRandomizationWin`. The original enabled-feature switch and every other
default argument remain. It adds no `--enable-automation`, arbitrary launch
flags, browser context or fixture executable override.

Portable tests call the pinned argument generator without launching a browser.
They check the exact final argument transformation, rejected drift/overrides,
opt-out identity, NetLog composition and the five-step workflow scope. The
existing Windows evidence tests still enforce independent scheduling and fatal
failures. One worker, zero retries, assertions, screenshot tolerances, test
order, fixture commands/endpoints, startup deadlines and job limits are
unchanged. Ordinary CI still builds and launches its fixture normally.

Run the portable checks with:

```sh
node --test tests/helpers/windows-ci-port-randomization.test.mjs tests/helpers/windows-visual-evidence.test.mjs tests/helpers/visual-netlog.test.mjs
```

Portable success does not qualify the mitigation. It needs the complete
ordinary Windows core job, every theme shard and required aggregate on the
exact candidate head, alongside all other checks. A failure remains a failure;
do not rerun an unchanged head to manufacture a green result or describe the
trial as permanent after one pass.
