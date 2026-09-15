# CI runtime

`Build and test` runs application verification and privileged qualification on
independent Ubuntu 24.04 runners. Each runner creates its own disposable database,
roles, credentials and seed data. The media and operations steps retain their
original order because guest provisioning, dispatch, intake and monitoring share
fixtures within that job.

The `rust-and-postgres` check succeeds only when both `rust-and-browser` and
`media-and-operations` succeed. A failed, cancelled or skipped child cannot make
that check pass. `visual-windows` retains every Windows screenshot and browser
check. No test is selected out, retried automatically or given a weaker assertion
by this split.

Both build and monitoring workflows run on pull requests, pushes to `main`, tags
and manual dispatch. Feature branches can use a pull request or manual dispatch
without running duplicate push and pull-request jobs for the same change. New
runs cancel superseded runs within the same workflow, event and PR/ref.

## Dependency caches

The pinned Rust cache action stores dependency artifacts separately for each job,
operating system, architecture and Rust environment. Keys also include the root
`Cargo.toml`, whose workspace dependency and profile settings affect every crate.
Workspace outputs and incremental artifacts are pruned before saving; installed
Cargo binaries are excluded. Valid dependency artifacts can be saved after a test
failure. Privileged Cargo commands disable incremental compilation explicitly,
and the qualification job returns build/cache ownership to the runner before the
cache action prunes and saves it.

Node setup caches npm downloads using `package-lock.json`. Every consuming job
still runs `npm ci --ignore-scripts`. `scripts/verify.sh` installs dependencies once
and, in CI, installs Chromium before Cargo tests that invoke the browser. Local
verification retains the existing separately installed browser prerequisite.

Databases, `.local` credentials, browser profiles, test results and VM fixtures
are not cached. Browser installation and system-package setup still run normally.

## Baseline and comparison

Successful runs on September 15, 2026, before this change:

| Run | Linux job | Application verification step | Later qualification steps |
| --- | --- | --- | --- |
| [PR 34981142168](https://github.com/frankischilling/26chan/actions/runs/34981142168) | 31m 57s | 19m 06s | 10m 55s |
| [Push 34981137738](https://github.com/frankischilling/26chan/actions/runs/34981137738) | 36m 39s | 23m 15s | 11m 12s |

Compare completed successful runs using the whole workflow duration, both Linux
child durations and cache hit/save logs. The short aggregate check is not a
replacement for the old Linux runtime measurement. Report cold and warm cache
results separately; early failures and skipped qualification are not speedups.
