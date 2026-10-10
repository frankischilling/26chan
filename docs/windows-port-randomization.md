# Windows Chromium port-randomization contrast

The separate `windows-port-randomization.yml` workflow runs one matched pair of
full theme shard 7. It enables `TcpPortRandomizationWin` in one arm and disables
it in the other. Required `ci.yml`, production features and OS settings stay
unchanged. A single passing arm cannot qualify a fix. If both arms pass, the
comparison is inconclusive.

## Why this feature

The current lock pins Playwright 1.62.0 and Chromium headless shell
151.0.7922.34, revision 1234. Chromium's
[pinned feature definition](https://github.com/chromium/chromium/blob/151.0.7922.34/net/base/features.cc#L257-L263)
enables the Windows port-randomization feature by default, with Windows 11 22H2
as its minimum. Build 26100 maps to
[Windows 11 24H2](https://github.com/chromium/chromium/blob/151.0.7922.34/base/win/windows_version.cc#L396-L405).

[`UseTcpPortRandomization`](https://github.com/chromium/chromium/blob/151.0.7922.34/net/socket/tcp_socket_win.cc#L105-L110)
checks both the feature and the OS minimum. The
[connect implementation](https://github.com/chromium/chromium/blob/151.0.7922.34/net/socket/tcp_socket_win.cc#L1046-L1078)
requests `SO_RANDOMIZE_PORT` just before `connect()` and deliberately ignores the
option call's return value. Verified launch features therefore do not prove that
Windows accepted the option on any particular socket.

The [native randomized dual-stack probe](windows-socket-diagnostics.md#native-dual-stack-result)
already reproduced error 10055 during an IPv6 connect without Chromium. Its
cleanup counts balanced, and qualification correctly failed. Neither that result
nor this browser contrast establishes which Windows resource failed or whether
the earlier browser failures have the same cause.

## Matched workload and fixture

The workflow builds the unchanged `visual-fixtures` example once with Rust
1.94.0. It uploads that exact executable with a manifest containing its SHA-256,
byte count, candidate head, workflow run and attempt. Both fresh Windows runners
download the same artifact and verify the executable before use. Neither arm
rebuilds the fixture.

A strict diagnostic-only `webServer` override executes that verified fixed-path
binary with no application arguments. Its inherited environment, readiness URL,
listener endpoints, no-reuse setting and 120-second startup deadline remain
unchanged. Ordinary runs retain `cargo run -p board-public --example
visual-fixtures --locked`.

Each arm keeps the original `playwright.themes.config.js`, shard `7/8`, output
directory, one worker, zero retries, assertions and 30-second test deadlines.
The existing discovery guard checks the full suite and all eight partitions
before the arm runs. The contrast reporter requires all 148 shard-7 cases in
their discovered order. Missing, skipped, duplicated, retried or interrupted
cases reject qualification. The original child exit remains authoritative,
including its nonzero status. A separate always-run job checks the matched pair.

Both arms must match on exact candidate head, run and first attempt; fixture
hash and byte count; actual browser executable hash; Playwright/browser versions;
ordered case digest; and runner image, full Windows build, architecture and Node
version. Missing values cannot match by both being absent. A runner-image rollout
between arms makes the pair unavailable.

## Feature verification and retention

Playwright already supplies enable- and disable-feature switches. Its
`ignoreDefaultArgs` filters the complete final argument list, including added
arguments. The enabled arm removes only the original enable-feature switch and
replaces it with the original list plus `TcpPortRandomizationWin`. The disabled
arm does the equivalent for the disable-feature switch. The opposite original
switch remains intact. Removing both originals, adding a second competing switch
or filtering out the replacement would invalidate the comparison.

The checked Playwright core bundle has SHA-256
`3258d1cf334c6afc95f22aa9c292436cb976b391e0437f1359c83b84f0cb9d66`.
The helper rejects a changed bundle, browser pin, existing caller launch options,
external connection/instrumentation overrides and debug/protocol logging,
including npm aliases and Windows case variants. This prevents a normalized
Chromium command line from concealing changed Playwright defaults.

Before any test context is created, each browser's worker calls
`Browser.getBrowserCommandLine`. It checks the actual executable and normalized
feature lists in memory. Retained proof contains only fixed booleans, counts,
version, worker ordinal and content hashes. The proof never copies the raw CDP response or its arguments and profile paths
into errors or artifacts. Session creation,
the command and detach each have a five-second diagnostic bound. Failure rejects
the proof without retrying a test or relaxing its deadline.

Executable hashing and CDP verification add host load and affect scheduling.
Results apply to this instrumented pair. Each executable read is bounded at
512 MiB, metadata at 4 KiB and each arm's proof at 64 KiB. At most 148 browser
proofs are admitted. The runner job has a 30-minute ceiling, as in the original
theme job.

The ordinary bounded Default-mode NetLog still runs. This workflow uploads only
an admitted diagnostic copy: it removes precisely
`constants.clientInfo.command_line` when present and accepts the current empty
`clientInfo` object. Unknown client metadata or malformed input makes the copy
unavailable. Events, source dependencies, clocks, enum dictionaries and
`polledData` retain their parsed values. Each retained copy has a verified hash,
byte count and redaction flag. At most four 12 MiB copies are retained, matching
the existing limits. A failed arm requires an admitted capture. Raw original
NetLogs and trace archives are not uploaded by this workflow. The existing
sanitized transport-stderr and fixture-lifecycle aggregates remain available.
Artifacts expire after three days.

The download action is pinned to
[`actions/download-artifact` v8.0.1](https://github.com/actions/download-artifact/releases/tag/v8.0.1),
commit `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c`. The official release and commit
were verified before adding the pin.

## Admission and interpretation

A narrow pull-request path trigger lets the new workflow qualify before it is
on the default branch. Manual dispatch becomes available after it reaches the
default branch. Admission queries this workflow's runs for the exact head and
allows only its first run, attempt 1. An earlier run, missing history or an
incomplete history response rejects admission. Do not rerun an unchanged
experiment or treat a rerun as a new observation.

Read the pair summary with both original exits and retained error evidence:

- Both arms passing is inconclusive about the intermittent failure.
- An error-10055 failure in one arm and a passing other arm is one matched
  observation. It does not establish a production fix or an OS root cause.
- Missing feature proof, changed binaries, different runners, incomplete
  workloads or unavailable failure captures reject pair qualification.

Portable checks use `node --test tests/helpers/windows-port-contrast.test.mjs`.
They cover feature-switch replacement and drift, private-input rejection,
shared-fixture binding, workload and runner mismatches, missing proof in both
arms, nonzero exits and the narrow NetLog redaction. Actual Windows/CDP execution
still needs exact-head hosted qualification. Every exact-head check must be
green before merge; a passing pair-check job does not erase a failed arm or a
failed required check.

## First hosted preflight failure

At `b9c841a`, [run 38078132595](https://github.com/frankischilling/26chan/actions/runs/38078132595)
passed admission, the shared fixture build and full shard discovery. Both arms
then stopped in preflight before tests or browser proofs were created. The pair
remains failed and provides no feature comparison.

The recorded runner image was `windows-2025-vs2026` version `20260925.250.1`,
with Windows build `26100.33438`. Its
[exact image template](https://github.com/actions/runner-images/blob/win25-vs2026/20260925.250/images/windows/templates/build.windows-2025-vs2026.pkr.hcl)
runs the [Selenium installer](https://github.com/actions/runner-images/blob/win25-vs2026/20260925.250/images/windows/scripts/build/Install-Selenium.ps1),
which sets the machine-level `SELENIUM_JAR_PATH` inventory marker. The original
environment guard rejected every `SELENIUM_` variable, including that marker.
The pinned Playwright bundle has no consumer for it; its Selenium launch inputs
are `SELENIUM_REMOTE_URL`, `SELENIUM_REMOTE_CAPABILITIES` and
`SELENIUM_REMOTE_HEADERS`.

The guard now permits only that inert JAR marker. Remote-launch inputs, unknown
Selenium overrides and debug aliases remain rejected. All other admission checks
remain in place. A preflight failure reports one fixed stage name: environment, binding,
runner, fixture, browser or output. No environment value, exception, path or
argument is included. These changes repair a reproduced preflight bug; another
exact-head hosted qualification is still required.
