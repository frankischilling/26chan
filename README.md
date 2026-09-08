# Paperboard

A Rust imageboard rewrite in development. The current milestone runs persisted text boards, threads, replies, password deletion, reporting, catalog pages and a documented subset of the public read-only JSON API.

This is not a production-ready release. Media processing and WebAuthn staff tooling are absent. Uploads cannot be enabled. Visual snapshots cover this project's synthetic pages; they do not establish visual parity with a reference site.

## Run locally

Requirements: Rust 1.94.0 (selected by `rust-toolchain.toml`), PostgreSQL 16, Node 24 or newer, and Playwright's pinned Chromium. The setup below uses Windows PowerShell plus Ubuntu 24.04 in WSL. Linux uses the same database scripts with `sudo` and sources `.local/database.env` instead of `.local/database.ps1`.

```powershell
# Once, in Ubuntu/WSL:
wsl -d Ubuntu -- bash -lc 'apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y postgresql-16 postgresql-client-16 openssl'
wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/dev-db.sh

# In the project root:
. .\.local\database.ps1
cargo run -p board-store --bin board-migrate --locked
wsl -d Ubuntu -- bash -lc 'cd /mnt/c/Users/imike/4chan-rewrite && source .local/database.env && bash scripts/seed.sh'
Remove-Item Env:MIGRATION_DATABASE_URL
cargo run -p board-public --locked
```

Open `http://127.0.0.1:3000/`. `/demo/` has fixed synthetic posts; `/test/` accepts new threads. Adapt the WSL path if the checkout is elsewhere. The database script creates a disposable cluster on port 55432, generates local credentials, and refuses to overwrite an existing environment. It does not change the installed default PostgreSQL cluster. Credentials and database backups stay under ignored `.local/` paths.

The public process receives only `DATABASE_URL`. `MIGRATION_DATABASE_URL` belongs to the operator CLI. For a standalone development server, unset it after migration: `Remove-Item Env:MIGRATION_DATABASE_URL`. Do not use development environment files in production.

## Verify

Stop a manually running server before browser tests; Playwright starts and stops its own server.

```powershell
. .\.local\database.ps1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --all-features --locked
npm.cmd ci --ignore-scripts
npx.cmd playwright install chromium
npm.cmd test
wsl -d Ubuntu -- bash /mnt/c/Users/imike/4chan-rewrite/scripts/restore-exercise.sh
```

Database tests require real public and migration test URLs and fail if they are unavailable. Ordinary `cargo test` does not enable them. Browser baselines currently cover Windows and Chromium 151.0.7922.34. Never accept changed screenshots without inspecting them. The Linux CI job runs browser behavior; the Windows job compiles the production views in a test-only fixture server for visual comparison.

## Project records

- [Compatibility matrix and exceptions](docs/compatibility.md)
- [Architecture and trust boundaries](docs/architecture.md)
- [Deployment and operations](docs/operations.md)
- [Verification record](docs/verification.md)
- [Launch blockers and remaining work](docs/readiness.md)
- [Dependency maintenance](docs/dependencies.md)

The rewrite has no configured GitHub remote. The nested `4chan-old` checkout is excluded and unchanged. Its source was not used to implement or specify this application. A publication target and the separately referenced design brief remain unresolved.
