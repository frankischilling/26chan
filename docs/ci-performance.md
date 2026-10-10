# CI runtime

`Build and test` runs Rust verification, two Linux browser suites, and privileged
qualification on independent Ubuntu 24.04 runners. Every runner creates its own
disposable database, roles, credentials, and seed data. Browser suites keep one
worker and zero retries. The media and operations steps run in their established
order because guest provisioning, dispatch, intake, and monitoring share fixtures
within that job.

The `rust-and-postgres` check requires `rust-and-browser`, both `browser-suites`
matrix entries, and `media-and-operations` to succeed. Failed, cancelled, or skipped
lanes fail the aggregate. On Windows, `visual-windows-core` and all eight
`visual-windows-themes` shards start independently; `visual-windows` requires both
results. The core job still collects its independent screenshot suites after
another qualification fails, provided browser installation and fixture
compilation succeeded.

## Local and CI verification

`bash scripts/verify.sh` runs the complete local sequence in its existing order.
CI selects one of these explicit modes:

| Mode | Work |
| --- | --- |
| `rust` | Source references, generated assets, runner guards, formatting, Clippy, workspace builds and tests, and the explicit staff upload and fixture tests |
| `browser-content` | Math and settings checks, drawing, polls, blotter, page identity, search, posting policies, quotes, images, file labels, spoilers, and board flags |
| `browser-interactions` | Display, updater, navigation, embeds, settings transfer, quick reply, catalog and watcher behavior, remaining posting policies, and staff browser tests |

Both browser modes build the workspace binaries and examples, build the quota
fixture with its browser feature, and run the two dedicated deletion fixture
tests before starting their browser workloads. They use the existing npm
commands, including the generated-file checks and Node tests inside those
commands. The full workspace test suite runs in the Rust lane and remains
required by the aggregate.

Every mode requires development mode and all seven disposable database role
URLs. Unknown modes and extra arguments fail before executing verification
commands. The Rust lane also installs Chromium because some Rust tests invoke a
real browser. The drawing and math commands retain their five-minute bounds.

Both build and monitoring workflows run on pull requests, pushes to `main`,
tags, and manual dispatch. New runs cancel superseded runs within the same
workflow, event, and PR/ref.

## Dependency caches

The pinned Rust cache action includes the toolchain, operating system,
architecture, compiler environment, manifests, lockfile, and Cargo configuration
in its cache identity. The existing Rust job writes its dependency cache; the
Linux browser lanes restore that same namespace without competing to save it.
The Windows core job uses the explicit namespace already restored by theme and
diagnostic jobs, and remains its only writer.

Workspace outputs and incremental artifacts are pruned before saving; installed
Cargo binaries are excluded. Valid dependency artifacts can be saved after a
test failure. Privileged Cargo commands disable incremental compilation
explicitly, and the qualification job returns cache ownership to the runner
before saving.

Node setup caches npm downloads using `package-lock.json`. Every consuming
runner still executes `npm ci --ignore-scripts`. Databases, credentials, browser
profiles, test results, and VM fixtures are not cached. System packages and
Chromium are installed on each runner.

## Runtime comparison

The successful [poll-voting run 38022559067](https://github.com/frankischilling/26chan/actions/runs/38022559067)
on October 10, 2026 is the baseline for this scheduling change:

| Measurement | Before splitting the application suites |
| --- | --- |
| Whole workflow | 54m 48s |
| Linux Rust and browser job | 54m 41s |
| Its verification-script step | 52m 47s |
| Media and operations | 19m 51s |
| Slowest Windows theme shard | 5m 50s |
| Windows core after waiting for themes | 8m 52s |

Compare complete successful runs using workflow duration, every lane's duration,
and cache hit/save logs. Record cold and warm caches separately. The short
aggregate checks do not measure the workload, and failed or skipped tests do not
count as a speed improvement.
