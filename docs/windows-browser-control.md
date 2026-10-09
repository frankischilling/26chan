# Windows browser failure control

This opt-in experiment asks whether a second Chromium process can make a fresh connection after the original Windows theme runner reports `ERR_NO_BUFFER_SPACE`. It does not fix that error or identify its cause.

The native receive/connect experiment completed all four arms without reproducing 10055. Those results cover its bounded workload, not Chromium's process and socket lifecycle. This control keeps the actual pinned browser in the experiment.

## What changes

The separate Windows workflow runs all eight original theme shards on independent runners. Each shard keeps its existing arguments, one worker, no retries, and unchanged assertions. A failure in one matrix job does not cancel the others. Required CI jobs remain unchanged. The diagnostic has an 18-minute owned-run safety bound inside a 20-minute execution step. Its separate 60-minute job ceiling leaves room for bounded cold toolchain setup and evidence cleanup; it does not increase any original test timeout. A second Node process owns one additional Chromium process, one context, and an unused page. It makes no warm-up request.

The first eligible failure from an original test context, for the exact fixture origin `http://127.0.0.1:3000`, triggers one navigation to `http://127.0.0.1:3000/readyz`. The original failure remains fatal regardless of the control result. Later failure notifications cannot trigger another navigation. A run without an eligible failure reports no trigger; it is not evidence that the control reproduced or resolved the problem.

Earlier transport failures have appeared on multiple shards, so restricting this control to shard 7 can miss an eligible failure. Expanding coverage does not classify every visual assertion failure as a transport failure; the exact-origin error trigger remains required.

The extra browser changes host load. Results apply to this instrumented run and must not be treated as equivalent to uninstrumented CI.

## Process and evidence separation

The control runs outside Playwright's test workers. Creating another browser inside a test worker can inherit that worker's decorated launch options, including its NetLog path, and can enter its tracing and context-cleanup machinery. A dedicated process avoids those shared defaults and survives replacement of a failed worker.

Control evidence lives under `test-results/windows-browser-control-N`, separate from the original `test-results/windows-themes-N` output, where N is exactly one digit from 1 through 8. Artifact names include that shard to avoid collisions. Both summary and ownership records use schema 2 and carry the same shard identity; the evidence check rejects missing, invalid, or mismatched identities and output paths. Playwright can clean the original project output when starting its runner; control output must not live there. Primary and control captures also have separate admission limits, so the control cannot consume a primary failure-capture slot.

The launcher contains its descendants in an owned Windows Job Object and attempts graceful browser cleanup. Once the coordinator exits, it allows up to 250 ms for job accounting to settle within the existing five-second cleanup budget. It still requires proof that the owned job has zero active processes. If any remain, it terminates that exact job and waits only for the remaining budget. It never terminates processes by executable name. Forced termination, missing or late cleanup proof, and incomplete logging cannot qualify complete control evidence.

Only admitted structured summaries and a complete, bounded Default-mode NetLog are retained. Pending chunks, private raw child output, arbitrary exception messages, cookies, storage, request bodies, and response bodies are not diagnostic artifacts. Existing original-shard evidence keeps its existing rules.

## Interpreting a result

A successful HTTP response alone is insufficient. The control capture must link the fixed readiness request to a fresh TCP connection attempt at `127.0.0.1:3000`. Connection reuse, missing source dependencies, incomplete capture, or a race with fixture teardown makes that evidence inconclusive. One navigation can contain multiple internal Chromium attempts; preserve and report them rather than calling it one socket attempt.

Parent receipt-to-dispatch and receipt-to-completion times describe observation lag on one monotonic clock. Primary and control NetLog clocks and source IDs remain separate. Do not subtract their raw timestamps to claim simultaneous activity or the exact delay from the original socket failure.

A successful fresh control connection shows that the other process could connect by that later interval. It cannot rule out a transient failure at the original instant. Another 10055 in the control supports failure across processes during the observed interval, but does not establish an operating-system cause. Both requests still use the same fixture process and runtime; this is not an independent-server comparison.

The original runner's result remains authoritative. Normal test exits retain their code. A signaled exit is recorded with its signal and returned as the existing normalized nonzero code; this Windows Job Object wrapper does not re-deliver the signal to itself. Diagnostic setup, collection, and cleanup problems are recorded separately and never turn an original failure into a passing test. A separate evidence check rejects unavailable or incomplete qualification even when the original tests passed.

A green diagnostic workflow can mean all eight shards completed with valid no-trigger, inconclusive evidence. It does not show that the socket cause is fixed. Interpret each shard separately, including its original test outcome and whether the control actually ran.
