# Local validation and remaining qualification

2026-10-08, Linux cloud workspace. No browser or Windows fixture was launched.

Passed:

- `node --test validate-output.test.mjs`: 37 tests, zero failures. Covers valid and failing results, preserved 10055, exact schema, bounds, counters, ownership identities, incomplete output, missing cleanup, required successful stages, and hidden option/connect/cleanup failures. Added adversarial controls cover appended/select 10055, close-before-create, global/phase deadline overruns, option Boolean/length mismatch, lifecycle ordering and unsupported result/error combinations.
- `g++ -std=c++17 -Wall -Wextra -Werror response-parser.test.cpp -o /tmp/26chan-response-parser-test` and the resulting binary: passed. Covers all partial-response boundaries and status, body, duplicate-length, transfer-encoding and size rejection.
- Read-only source/lifecycle review identified and corrected a timer cast hazard, missing second Chromium-default event handle, delayed per-socket deadline, and fixture ownership recheck gap. Deliberate deviations are documented rather than represented as exact browser equivalence.

Not executed or established:

- Windows SDK/cl compilation, including `/W4 /WX` and SDK support for `SO_RANDOMIZE_PORT`.
- PowerShell 7 parsing/execution or hosted fixture ownership checks.
- Native socket operations, option readback, failure/cleanup paths on Windows, timing behavior, and both controlled variants.
- Reproduction of 10055, dependence on port randomization, or any corrective effect.

No changes were made to the existing checkout, required CI, browser flags, baselines, network/security settings or port ranges. No diagnostic artifacts were published and no CI was activated. The original required-CI failure remains unresolved.
