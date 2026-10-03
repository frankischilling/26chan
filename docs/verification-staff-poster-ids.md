# Static staff ID qualification

This checkpoint extends draft PR #227 with the
[source static staff ID rules](source-staff-poster-ids.md). Complete original
board, feature and frontend parity remains the completion scope. Issues #216
and #222 stay open.

The reference is revision `545b7812d1849f7958d914950c91fdbbe38f6b22`.
The extractor pins `imgboard.php` at SHA-256
`caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`.
It evaluates only the pure static-label block with synthetic badge inputs
under PHP 8.3.6, a 16 MiB memory limit and five-second deadline. Output is
limited to 4 KiB and the fixture to 8 KiB. Its 14 cases include two `none`
controls; ordinary network IDs use their existing separate path.

Migration 0073 has SHA-384:

```text
8cec68b8c87908d4a36caab276dba2f69089a3fc249e42637bd121225b40c8ecb3987b6050e681fd30c4616c68d593fe
```

The fresh owned qualification database has all 64 migration files through
version 73, with the same checksum in its ledger. Applied migration bytes
remain unchanged. Migration 0073 changes the ID constraint and insertion
trigger, preserving historical fields, proof payloads, privacy policies,
function OIDs, owners and ACLs. Apply it before running the posting service.

## Executed checks

| Check | Result |
|---|---|
| Independent source regeneration | Passed; 14 cases |
| Populated 0073 upgrade | Passed; historical fields and source-bound proof preserved, then consumed with prepared trip and static ID; replay and forgery denied |
| Fresh role bootstrap | Passed; all 64 files, privacy and role guards retained |
| Strict workspace Clippy | Passed in 1 minute 18 seconds; all targets/features, warnings denied |
| Formatting | Passed |
| Staff HTTP/PostgreSQL suite | 18 passed in 3.24 seconds; no failures or ignored tests |
| Native Windows domain suite | 148 passed across 18 groups; no failures or ignored tests |
| Full Linux binary/example build | Passed in 6 minutes 9 seconds with debug symbols disabled |
| Full Linux workspace | Incomplete locally; WSL restarted during the restored-browser run |
| Generated assets and client regressions | Passed separately; 109 client unit/DOM checks, no failures or skipped tests |
| Public and separate staff browser regressions | Not completed for this follow-up locally |
| Hosted checks at the next published head | Required after publication |

The HTTP regression posts every badge with IDs on and off, without a poster
key or peer address. It checks saved fields through policy changes, both JSON
listeners, full/tail/index/catalog/archive responses and HTML badge headers.
Ordinary callers cannot forge a short static label; mismatched badge/label
updates fail the constraint. Existing proof expiry, permission, revocation,
lock-wait, name/trip and private-board checks remain enforced.

## Retained failures and qualification environment

The first populated-upgrade fixture omitted the trip GUC bound to its existing
source proof. Consumption correctly rejected the mismatch before ID assignment.
The fixture now supplies the same prepared trip as the posting transaction;
the proof's exact-field and 15-second expiry checks are unchanged.

The first staff suite on the rebuilt server exhausted its default 100
connections. PostgreSQL logged reserved-slot and too-many-client failures.
Increasing only the owned local qualification cluster to 300 connections made
all 18 cases pass. Runtime role deadlines, production configuration, assertions
and the explicit concurrent tasks remain unchanged. Failure logs are retained.

WSL restarted during initial migration compilation, the first full binary
build and subsequent workspace-test compilation. Its original disposable
PostgreSQL data under `/tmp` was lost.
Earlier result logs, checksums and private environment files are retained;
those old databases are no longer claimed to be live. The fresh version 73
cluster now stores data outside `/tmp`. After another restart, its ledger and
checksum still matched before work resumed. The interrupted build log is kept.

The resumed full Linux build disables debug symbols to reduce local build
memory use. It retains development/test optimization and debug assertions.
Its own service has a verified 3 GiB memory limit. Dependencies were rebuilt
after the interrupted runs. These local build settings change neither test
assertions nor runtime role deadlines.
Strict Clippy and the 18-case staff suite already passed with the normal local
profile. Hosted checks use their existing configuration.

A later background service passed its binary build, then was stopped during
test compilation. The journal records a clean service stop; the incomplete log
does not establish a successful workspace run or an out-of-memory failure.
Microsoft documents that [systemd services do not keep WSL alive](https://learn.microsoft.com/en-us/windows/wsl/systemd).
The resumed runner retains a foreground WSL session, records its exit code and
keeps the completed service status available. The separate three-case public
post-identity diagnostic passed in 40.41 seconds; it does not substitute for
the complete workspace suite.

The foreground runner subsequently reached the media reader's real-browser
test and failed because its pinned Chromium executable was absent from the
old local cache. Its exit code 101 and test log are retained. Playwright's
pinned Chromium 151.0.7922.34 was restored under the persistent qualification
directory and passed an actual launch/page check. The complete workspace,
client and browser checks then resumed using that cache. The unchanged
binary/example build had already passed; its log remains separate from the
regression rerun. No browser version, test deadline or retry policy changed.

WSL restarted again during the restored-browser workspace run. Its transient
service disappeared and no final exit record was written. The partial test log
is retained and is not counted as a workspace pass. Hosted build/test must
qualify the complete Rust and browser suites at the published follow-up head;
the preceding green head is only historical evidence.

The separate client run initially selected the empty default browser cache.
After selecting the restored qualification cache, all 109 unit/DOM checks and
generated-bundle checks passed. Both logs remain available. This finite client
suite does not qualify the complete public or staff browser flows.

The preceding `d19f471` checkpoint passed all hosted workflows, as recorded in
[staff JSON qualification](verification-staff-json.md). Those results do not
qualify the static-ID follow-up. Full source JSON, unbadged authorized posting,
Pass benefits and the remaining board, media and frontend scope stay open.
