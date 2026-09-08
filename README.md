# 26chan

A 4chan clone rewritten in Rust, named for iron's atomic number: 26.

Built with Axum, Askama, PostgreSQL and SQLx. The application supports text boards, threads, replies, password deletion, reporting, catalog pages and a subset of the public read-only JSON API. Core browsing and posting work without JavaScript.

Development is ongoing. Media uploads and WebAuthn staff tooling are not implemented, and this is not a production-ready release. See the [compatibility matrix](docs/compatibility.md) for supported behavior and known differences.

## Getting started

The development scripts support Ubuntu 24.04, including Ubuntu running in WSL. Run the following commands in an Ubuntu terminal as a regular user with `sudo` access.

Requirements:

- Rust installed through rustup; `rust-toolchain.toml` selects Rust 1.94.0.
- PostgreSQL 16 and its command-line tools.
- Node.js 24 or newer and npm for browser tests.

```bash
sudo apt-get update
sudo apt-get install -y git build-essential pkg-config postgresql-16 postgresql-client-16 openssl

git clone https://github.com/frankischilling/26chan.git
cd 26chan

sudo bash scripts/dev-db.sh
sudo chown "$(id -u):$(id -g)" .local .local/database.env .local/database.ps1
source .local/database.env
cargo run -p board-store --bin board-migrate --locked
bash scripts/seed.sh

unset MIGRATION_DATABASE_URL
cargo run -p board-public --locked
```

Open `http://127.0.0.1:3000/`. `/demo/` contains sample posts, and `/test/` accepts new threads.

The setup script creates a separate disposable database on port 55432 and generates credentials under the ignored `.local/` directory. It refuses to overwrite an existing setup. Database files live under `/tmp`, so this setup is unsuitable for durable storage. See [operations](docs/operations.md) for database lifecycle and backup instructions.

For later runs, load the generated environment and remove the migration credential before starting the public server:

```bash
source .local/database.env
unset MIGRATION_DATABASE_URL
cargo run -p board-public --locked
```

## Testing

Stop a manually running server before browser tests; Playwright starts and stops its own server. Run these commands from the checkout:

```bash
source .local/database.env
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --all-features --locked
npm ci --ignore-scripts
npx playwright install --with-deps chromium
npm run test:behavior
sudo bash scripts/restore-exercise.sh
```

Database tests require the migrated, seeded development database and fail if it is unavailable. Ordinary `cargo test` does not enable those tests; use `--all-features` as shown above. The browser runner starts its public server without inheriting migration credentials.

Screenshot baselines currently target Windows and the pinned Chromium 151.0.7922.34. Linux runs browser behavior tests. To compare screenshots on Windows without a database, install the Rust and Node prerequisites, then run from the checkout in PowerShell:

```powershell
npm ci --ignore-scripts
npx playwright install chromium
cargo build -p board-public --example visual-fixtures --locked
$env:VISUAL_FIXTURE_SERVER = '1'
npm run test:visual
Remove-Item Env:VISUAL_FIXTURE_SERVER
```

Inspect screenshot differences before changing baselines. These snapshots use synthetic project pages and do not establish visual parity with 4chan. Browser and font details are recorded in the [reference manifest](docs/reference-manifest.json).

## Documentation

- [Compatibility matrix and exceptions](docs/compatibility.md)
- [Architecture and trust boundaries](docs/architecture.md)
- [Deployment and operations](docs/operations.md)
- [Verification record](docs/verification.md)
- [Launch blockers and remaining work](docs/readiness.md)
- [Dependency maintenance](docs/dependencies.md)
