# Windows browser failure control

This opt-in experiment asks whether a second Chromium process can make a fresh connection after an original Windows browser test reports `ERR_NO_BUFFER_SPACE`. It does not fix that error or identify its cause.

The native receive/connect experiment completed all four arms without reproducing 10055. Those results cover its bounded workload, not Chromium's process and socket lifecycle. This control keeps the actual pinned browser in the experiment.

The later [randomized dual-stack probe](https://github.com/frankischilling/26chan/actions/runs/38002593263/job/114063950992) did capture native error 10055 during an IPv6 connect. Its socket and event counts balanced at cleanup, and its validator correctly rejected qualification. This separate result does not identify the Windows resource involved or establish the cause of Chromium's earlier failures. The [recorded result](windows-socket-diagnostics.md#native-dual-stack-result) retains the counts and limits of that evidence.

## What changes

The separate Windows workflow runs all eight original theme shards and one complete media-visual suite, each on its own runner. The media suite uses the same `playwright.media-visual.config.js` and output directory as the required attachment visual check. The original arguments, one worker, retries, assertions, and test deadlines remain intact. A failure in one matrix job does not cancel the others. Required CI jobs remain unchanged. The diagnostic has an 18-minute owned-run bound inside a 20-minute execution step, with a separate 60-minute job ceiling for installation, building, and cleanup. A second Node process owns one additional Chromium process, one context, and an unused page. It makes no warm-up request.

The first eligible failure from an original test context, for the exact fixture origin `http://127.0.0.1:3000`, triggers one navigation to `http://127.0.0.1:3000/readyz`. The original failure remains fatal regardless of the control result. Later failure notifications cannot trigger another navigation. A run without an eligible failure reports no trigger; it is not evidence that the control reproduced or resolved the problem.

Earlier transport failures appeared in theme shards and the attachment visual suite. The media-visual control includes `tests/media-visual/native-images.spec.js`, where the post-menu test failed after the thread-watcher script request returned `ERR_NO_BUFFER_SPACE`. The trigger is installed by the `visual-diagnostics.js` test fixture. Tests using the ordinary Playwright fixture are not trigger sources. Every control still requires the exact-origin socket error, so a visual assertion failure alone does not start it.

The extra browser changes host load. Results apply to this instrumented run and must not be treated as equivalent to uninstrumented CI.

## Process and evidence separation

The control runs outside Playwright's test workers. Creating another browser inside a test worker can inherit that worker's decorated launch options, including its NetLog path, and can enter its tracing and context-cleanup machinery. A dedicated process avoids those shared defaults and survives replacement of a failed worker.

Theme control evidence lives under `test-results/windows-browser-control-N`, separate from `test-results/windows-themes-N`, where N is exactly one digit from 1 through 8. The media run uses `test-results/windows-browser-control-media-visual` and `test-results/windows-media-visual`. All directories and artifact names identify their exact run. Summary and ownership records use schema 3 and carry the same case and suite identity. The checker rejects mismatched paths and identities. Control captures stay outside Playwright's project output and do not consume original failure-capture slots.

The launcher contains its descendants in an owned Windows Job Object and attempts graceful browser cleanup. Once the coordinator exits, it allows up to 250 ms for job accounting to settle within the existing five-second cleanup budget. It still requires proof that the owned job has zero active processes. If any remain, it terminates that exact job and waits only for the remaining budget. It never terminates processes by executable name. Forced termination, missing or late cleanup proof, and incomplete logging cannot qualify complete control evidence.

The sidecar retains a complete, bounded Default-mode Chromium NetLog after closing the browser, even if no trigger occurred. The checker reads the actual capture and verifies its required schema before accepting diagnostic setup. A missing, oversized, malformed, or incomplete capture fails qualification; an absent pending file can no longer count as successful cleanup. The original runner's bounded stderr record must also confirm complete capture, no setup refusal, no dropped or unavailable probe evidence, and positive initialization markers from both coordinator and test worker. Unhealthy records fail qualification without changing the original test result. Pending chunks, private raw child output, arbitrary exception messages, cookies, storage, request bodies, and response bodies are not diagnostic artifacts. Ordinary theme diagnostics retain their existing behavior.

## Interpreting a result

A successful HTTP response alone is insufficient. The control capture must link the fixed readiness request to its HTTP stream job and socket, along with a fresh TCP connection attempt at `127.0.0.1:3000`. Chromium can record the socket connection and its job binding before recording the request-to-job binding. The checker follows the actual source dependencies and the attempt's timing in that NetLog. Connection reuse, missing dependencies, incomplete capture, or a race with fixture teardown makes that evidence inconclusive. One navigation can contain multiple internal Chromium attempts; the record preserves their count.

Parent receipt-to-dispatch and receipt-to-completion times describe observation lag on one monotonic clock. Primary and control NetLog clocks and source IDs remain separate. Do not subtract their raw timestamps to claim simultaneous activity or the exact delay from the original socket failure.

A successful fresh control connection shows that the other process could connect by that later interval. It cannot rule out a transient failure at the original instant. Another 10055 in the control supports failure across processes during the observed interval, but does not establish an operating-system cause. Both requests still use the same fixture process and runtime; this is not an independent-server comparison.

The original runner's result remains authoritative. Normal test exits retain their code. A signaled exit is recorded with its signal and returned as the existing normalized nonzero code; this Windows Job Object wrapper does not re-deliver the signal to itself. Diagnostic setup, collection, and cleanup problems are recorded separately and never turn an original failure into a passing test. A separate evidence check rejects unavailable or incomplete qualification even when the original tests passed.

A green diagnostic workflow can mean all eight theme shards and the media suite completed with healthy, no-trigger evidence. Such results remain inconclusive about the socket failure. A real triggered comparison still needs to occur before this experiment can answer its question. Read each case alongside its original result; a passing diagnostic job does not turn a failed required Windows check into a pass.
