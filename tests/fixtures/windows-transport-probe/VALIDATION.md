# Diagnostic validation status

Initial local checks on 2026-10-08 ran on Linux without a browser or Windows fixture.

Passed:

- `node --test validate-output.test.mjs`: 37 tests, zero failures. Covers valid and failing results, preserved 10055, exact schema, bounds, counters, ownership identities, incomplete output, missing cleanup, required successful stages, and hidden option/connect/cleanup failures. Added adversarial controls cover appended/select 10055, close-before-create, global/phase deadline overruns, option Boolean/length mismatch, lifecycle ordering and unsupported result/error combinations.
- `g++ -std=c++17 -Wall -Wextra -Werror response-parser.test.cpp -o /tmp/26chan-response-parser-test` and the resulting binary: passed. Covers all partial-response boundaries and status, body, duplicate-length, transfer-encoding and size rejection.
- Read-only source/lifecycle review identified and corrected a timer cast hazard, missing second Chromium-default event handle, delayed per-socket deadline, and fixture ownership recheck gap. Deliberate deviations are documented rather than represented as exact browser equivalence.

Still unqualified:

- Successful Windows SDK/cl compilation under `/W4 /WX` and native SDK behavior for `SO_RANDOMIZE_PORT`.
- Native socket operations, option readback, failure/cleanup paths on Windows, timing behavior, and both controlled variants.
- Reproduction of 10055, dependence on port randomization, or any corrective effect.

## First hosted attempt

Both variants in [run 37755261505](https://github.com/frankischilling/26chan/actions/runs/37755261505) reached the compiler after verifying fixture ownership. MSVC rejected the neutral `WSASocket` macro because it selected deprecated `WSASocketA`; `/WX` correctly made that a failure. Neither variant executed native sockets. Both retained complete compiler-failure status and confirmed child/fixture cleanup.

The source now calls `WSASocketW` explicitly, preserving the socket parameters without suppressing warnings. A new hosted run is required. Existing required CI, browser flags, baselines and persistent network settings remain unchanged. The original browser 10055 failure remains unresolved.
