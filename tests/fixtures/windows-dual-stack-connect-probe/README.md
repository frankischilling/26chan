# Windows dual-stack connect diagnostic

This opt-in profile exercises IPv6-first media connects with delayed IPv4 fallback and closes some IPv6 sockets before consuming their connect result. The launcher explicitly selects the owned visual fixture's `ipv4-only` media profile. Normal browser fixtures use explicit IPv4 and IPv6 loopback listeners for the same media router. Production code, browser assertions, and the fixture's `localhost:3004` origin are unchanged.

## Why this workload

The inspected PR #242 NetLog contained 465 connect starts. All 154 starts lacking a completion record were IPv6 localhost media attempts; their close times ranged from 199 to 317 ms, with a 302 ms median. In 126 dual-address jobs, fallback also had a 302 ms median. The first observed 10055 was on IPv6 before a succeeding IPv4 attempt; the second was on IPv4. These observations identify a workload to test. They do not establish the cause of 10055.

The fixture deliberately uses a localhost media origin for cookie isolation. The diagnostic retains the original IPv4-only bind so it continues to exercise IPv6 fallback; normal browser runs now cover both localhost address families. Missing IPv6 support fails normal fixture startup rather than silently falling back. The pooled transport profile uses an IPv4 public endpoint and consumes every connect completion, so it does not exercise this media fallback/close pattern.

## Fixed bounds and behavior

- 40 sequential groups, six logical attempts per group
- IPv6 media connect first; the IPv4 media connect starts at least 300 ms after its corresponding IPv6 connect record
- At most 12 live sockets and 480 connect starts, with no retries
- 700 ms per-group deadline, 30 s work deadline, 35 s cleanup deadline, and 40 s native-process watchdog
- Three lanes consume IPv6 completion events; three defer handling until socket close
- All lanes consume IPv4 completions, then close their IPv6 socket and IPv4 socket
- Group expiry fails the run and closes any resources still owned

The timer is a fixed 300 ms target with normal scheduler overshoot, not an adaptive delay. Socket setup and trace output can also add time. The probe performs no HTTP requests, send/receive calls, or overlapped I/O. Each matrix variant sets and reads back `SO_RANDOMIZE_PORT` on every socket. The plain variant writes false; the randomized variant writes true. Neither variant changes machine settings.

Before closing an unhandled connect, the probe performs a zero-time event wait. It separately records a signaled event, an unsignaled event, synchronous success/refusal, and a consumed completion. A signaled event may represent a completed refusal. Even an unsignaled event is only an observation of notification state. `closed_before_handling` never means that the connect was proven pending inside the kernel.

The consuming lanes record expected IPv6 connection refusals separately. Unhandled deferred completions have no inferred success/error status. A 10055 stored in an unconsumed completion would be invisible to this profile; success establishes only that no observed operation reported it. Any observed 10055, unexpected IPv4 refusal, setup error, close error, deadline, or ownership imbalance fails qualification. Every socket receives one close attempt; its event is released only after successful socket close. No socket API uses that handle afterward. A failed close retains its event until process exit and cannot qualify.

## Run on native Windows

The dedicated `Windows dual-stack connect diagnostic` workflow runs plain and randomized variants independently on `windows-2025`. It is path-scoped to this profile and can also be dispatched manually. It builds the existing visual fixture, runs the Node evidence tests, compiles with MSVC `/W4 /WX`, and invokes:

```powershell
./tests/fixtures/windows-dual-stack-connect-probe/run-hosted.ps1 -Mode plain
./tests/fixtures/windows-dual-stack-connect-probe/run-hosted.ps1 -Mode randomized
```

For a direct invocation, build `visual-fixtures.exe` first. The launcher discovers the installed MSVC environment and requires Node plus PowerShell 7. It refuses occupied fixture ports, starts only its own fixture, verifies the media listener's owner, executable and loopback bind before and after the probe, and stops only processes it created. Each child starts suspended, joins its own kill-on-close Job Object, and resumes only after containment succeeds. Cleanup checks the job's active-process count, including descendants after a root exits. The 40 s watchdog covers the native probe. MSVC discovery and environment setup each have a 20 s limit. Only PATH, INCLUDE, LIB and LIBPATH are exported from that setup. Compilation has a separate 120 s limit; fixture readiness has a 20 s limit and validation a 15 s limit. Watchdog termination of bootstrap, compiler, probe or validator children prevents qualification, even if their contained trees then exit. Planned fixture-server and ownership-test job shutdown is expected and must verify zero remaining owned processes. Cleanup failures remain unverified.

Evidence goes into a fresh invocation directory under `test-results/windows-dual-stack-connect`. The workflow uploads an explicit JSON evidence whitelist for that invocation and a candidate/runner identity file. Compiler and fixture output and arbitrary stderr stay outside the uploaded directory. Evidence retention is three days. A compiler failure is reported by phase and exit code; arbitrary compiler output is intentionally excluded.

## Evidence and validation

`SCHEMA.md` describes the fixed JSONL grammar and counter meanings. Socket ordinals encode group, lane and fixed endpoint family. No socket handles, PIDs, local ephemeral ports, raw addresses, HTTP content, or raw network traces appear in the native evidence. The validator independently checks ordering, timing, ownership, expected outcomes, resource counters and fixed bounds. It does not trust `complete` alone.

```sh
node --test tests/fixtures/windows-dual-stack-connect-probe/validate-output.test.mjs
node tests/fixtures/windows-dual-stack-connect-probe/validate-output.mjs path/to/output.jsonl
```

A complete run with zero close-before-handling exposure is inconclusive and exits nonzero. A qualifying run demonstrates only that this bounded native diagnostic completed without the checked failures and exercised unhandled-completion closes. It does not qualify browser behavior, prove a kernel cancellation race, eliminate the intermittent 10055 issue, or justify a production setting change.

At candidate `8da0f2c3b6f3627dbbd1101bade05a12ea82cb5d`, [hosted run 37913194848](https://github.com/frankischilling/26chan/actions/runs/37913194848) compiled and completed both native variants on Windows image `20260925.250.1`. Each recorded 480 connect starts, 240 IPv4 successes, 240 closes before completion handling, 480 socket/event opens and closes, and zero observed failures. Both verified the listener before and after execution, completed process cleanup, and passed independent evidence validation. Neither observed a ready-before-close completion or a consumed IPv6 refusal, so those branches remain unqualified by this run. The later fixture profile selection requires fresh hosted qualification. Synthetic Node traces test evidence controls only.

## Ownership-test correction

The first PR #244 hosted run ([37909599159](https://github.com/frankischilling/26chan/actions/runs/37909599159)) failed in the process-ownership test before the socket probe ran. The synthetic Node parent used `unref()` without `detached: true`. In the [pinned Node 24.14.0 libuv implementation](https://github.com/nodejs/node/blob/v24.14.0/deps/uv/src/win/process.c#L65-L71), non-detached children join a kill-on-parent-exit job, so the test did not establish a surviving descendant. The reported failure did not prove a containment defect.

The corrected fixture uses detached mode, which [Node documents as permitting Windows child survival after parent exit](https://nodejs.org/download/release/v24.14.0/docs/api/child_process.html#optionsdetached). Its [libuv creation path does not request job breakaway](https://github.com/nodejs/node/blob/v24.14.0/deps/uv/src/win/process.c#L988-L1035). The test waits for the child to acknowledge startup, verifies its retained process handle belongs to the exact enclosing Job Object, then allows the parent to exit. It requires the known live root and child before release, the surviving child afterward, incomplete tree status while that child lives, and verified termination through the owned job. Complete role enumeration accounts for any additional verified system console hosts. Readiness and exit waits are bounded; acknowledgement failure still closes the owned job. Child IDs remain in private temporary test files and never enter uploaded evidence.

The added Node checks cover these test-source contracts statically. The corrected ownership test and socket variants subsequently passed in run 37913194848, as described above.

The next hosted run, [37910796616](https://github.com/frankischilling/26chan/actions/runs/37910796616), received the startup acknowledgement but failed the combined pre-release live-child/membership/count assertion, again before the socket probe. That log did not identify which condition failed. The ownership test now emits at most three fixed-schema state records before its hard assertions: root/descendant exit and exact-job membership booleans, accounting-layout validity, and active/total/terminated job counts. It exports no process IDs, handles, paths or exception text. The retained-process membership requirements remain unchanged; the following run isolated the count mismatch described below.


Run [37911878210](https://github.com/frankischilling/26chan/actions/runs/37911878210) measured both known processes alive in the exact owned job, a valid accounting layout, and three active processes. It failed the previous exact-two assumption; the third process was not identified. The classifier now queries that job's [process list](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_process_id_list) with one fixed 16-entry buffer. It verifies every listed member is the retained root/child or a live exact-job member whose [queried executable image](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew) exactly matches the OS system directory's `conhost.exe`. Microsoft's [console-host design](https://github.com/microsoft/terminal/blob/main/doc/specs/%23492%20-%20Default%20Terminal/spec.md) documents that inbox image. Run 37913194848 subsequently verified the extra member as that exact system console-host image in both variants, retained the known child after root exit, and verified zero remaining processes after job termination.

Incomplete, duplicate, changing, unknown, unavailable or over-capacity role evidence fails. Accounting must match the full enumeration before and after classification. The test still requires one retained live child, the expected root state, and zero remaining job processes after termination. Only fixed role counts and completeness flags leave the classifier. Hosted synthetic classifier checks reject path impostors and inconsistent role counts; Node checks inspect these source contracts and do not establish native identity or containment.
