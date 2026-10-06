# 26chan

A 4chan clone rewritten in Rust, named for iron's atomic number: 26.

Built with Axum, Askama, PostgreSQL and SQLx. The application supports text boards, threads, replies, password deletion, reporting, catalog pages and a subset of the public read-only JSON API. Core browsing and posting work without JavaScript.

Development is ongoing. The explicit isolated development profile supports public uploads, processing status, persisted attachments, thumbnails and file deletion. Production uploads remain disabled pending qualification. A separate WebAuthn staff application handles report review, staff-badged text posting and moderation. This is not a production-ready release. See the [compatibility matrix](docs/compatibility.md) for supported behavior and known differences.

Migrations install [all original board definitions](docs/original-board-inventory.md),
with configured catalogs, archives, formatting, IDs and flags. Public database
credentials cannot read or write private `/j/` content. Special board workflows
and the remaining source features are tracked in
[#191](https://github.com/frankischilling/26chan/issues/191).

A [disposable Firecracker media profile](docs/firecracker.md) runs Rust PNG, [JPEG](docs/jpeg-media.md) and [single-frame GIF](docs/gif-media.md) decoders inside a per-job guest and validates bounded output. All published files are normalized PNGs; original uploads are never downloadable. [Publication commands](docs/media-approval.md) provide durable lease-fenced approval, interrupted-output reconciliation and a restricted reader. [HTTP intake](docs/media-intake.md) connects uploads to authenticated dispatch. Production host and storage qualification remain unfinished.

[Native image controls](docs/native-image-controls.md) add inline expansion, screen fitting, hover previews and image display preferences to approved attachments. They share the existing settings and quote ownership rules, with finite full-image loading and an explicit media-origin boundary.

[File headers and mobile captions](docs/file-presentation.md) use the released
client's filename cutoffs and size units. Full-name filters and inert filename
tooltips retain the uploaded display label while downloads remain normalized PNGs.

[Local dates and custom board navigation](docs/native-display.md) use the same
settings and preserve server output when disabled. The
[legacy action route](docs/legacy-actions.md) supports password deletion and a
script-free report form alongside posting.

[Index expansion](docs/native-thread-expansion.md) loads omitted replies without
replacing existing posts or drafts. [Thread statistics](docs/native-thread-stats.md)
show counts and page position from one database snapshot.

[Incremental page loading](docs/native-depager.md) keeps existing threads and
drafts while fetching later index pages. [Navigation controls](docs/native-navigation.md)
and [layout preferences](docs/native-layout.md) provide movable page navigation,
board selection, compact or centered threads, and an optional Tomorrow theme.

[Provider embeds](docs/native-embeds.md) load YouTube or SoundCloud players only
after an explicit click. [Custom CSS](docs/native-custom-css.md) supports saved
post colors, typography and spacing through a bounded editor. Provider frames
have fixed destinations; saved styles cannot import resources or cover controls.
[Settings export and restore](docs/native-settings-transfer.md) moves supported
preferences, filters and post styles through a saved link. Restoring requires
confirmation and checks for intervening changes in other tabs.

## Getting started

The development scripts support Ubuntu 24.04, including Ubuntu running in WSL. Run the following commands in an Ubuntu terminal as a regular user with `sudo` access.

Requirements:

- Rust installed through rustup; `rust-toolchain.toml` selects Rust 1.94.0.
- PostgreSQL 16 and its command-line tools.
- ICU 74.2 and PCRE2, supplied by maintained Ubuntu 24.04 packages for development and deployment.
- Node.js 24 or newer and npm for browser tests.

```bash
sudo apt-get update
sudo apt-get install -y git build-essential pkg-config postgresql-16 postgresql-client-16 openssl perl python3 nginx-light libicu-dev libpcre2-dev

git clone https://github.com/frankischilling/26chan.git
cd 26chan

sudo bash scripts/dev-db.sh
sudo bash scripts/dev-media-db.sh
sudo bash scripts/dev-media-reader-db.sh
sudo bash scripts/dev-intake-db.sh
sudo bash scripts/dev-monitor-db.sh
sudo bash scripts/dev-staff-db.sh
sudo chown "$(id -u):$(id -g)" .local .local/database.env .local/database.ps1
sudo chown "$(id -u):$(id -g)" .local/media.env .local/media.ps1
sudo chown "$(id -u):$(id -g)" .local/media-reader.env .local/media-reader.ps1
sudo chown "$(id -u):$(id -g)" .local/intake.env .local/intake.ps1
sudo chown "$(id -u):$(id -g)" .local/monitor.env .local/monitor.ps1
sudo chown "$(id -u):$(id -g)" .local/staff.env .local/staff.ps1
source .local/database.env
cargo run -p board-store --bin board-migrate --locked
bash scripts/seed.sh

unset MIGRATION_DATABASE_URL
cargo run -p board-public --locked
```

Open `http://127.0.0.1:3000/`. `/demo/` contains sample posts, and `/test/` accepts new threads.

The setup script creates a separate disposable database on port 55432 and generates credentials and a fresh development `POSTER_ID_KEY` in the ignored `.local/database.env` and `.local/database.ps1` files. Reloading these files preserves the same development identity key across server restarts. It refuses to overwrite an existing setup. Database files live under `/tmp`, so this setup is unsuitable for durable storage. See [operations](docs/operations.md) for database lifecycle and backup instructions.

For browser JSON clients on a separate origin, enable the optional [read-only API listener](docs/api.md) with `API_ORIGIN` and `API_BIND_ADDR`. It permits CORS from the configured board origin and shares the public process's database and resource limits.

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
source .local/media.env
source .local/media-reader.env
source .local/intake.env
source .local/monitor.env
source .local/staff.env
export APP_ENV=development
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --examples --bins --locked
cargo test --workspace --all-features --locked
cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked
npm ci --ignore-scripts
npx playwright install --with-deps chromium
npm run test:images-core
npm run test:display
npm run test:behavior
npm run test:staff
sudo bash scripts/test-comment-migration.sh
sudo bash scripts/test-role-bootstrap.sh
sudo bash scripts/test-media-approval-migration.sh
sudo bash scripts/restore-exercise.sh
```

Database tests require the migrated, seeded development database and fail if it is unavailable. Ordinary `cargo test` does not enable those tests; use `--all-features` as shown above. Browser launchers pass each service only its own credentials. The full Windows workspace build also needs Perl for vendored OpenSSL; see [staff setup](docs/staff.md).

Screenshot baselines currently target Windows and the pinned Chromium 151.0.7922.34. Linux runs browser behavior tests. To compare screenshots on Windows without a database, install the Rust and Node prerequisites, then run from the checkout in PowerShell:

```powershell
./scripts/install-windows-icu.ps1
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
- [Read-only API origin and browser contract](docs/api.md)
- [Architecture and trust boundaries](docs/architecture.md)
- [Deployment and operations](docs/operations.md)
- [Verification record](docs/verification.md)
- [API origin verification](docs/verification-api.md)
- [Comment character-limit verification](docs/verification-comment-limits.md)
- [Response admission verification](docs/verification-response-admission.md)
- [Launch blockers and remaining work](docs/readiness.md)
- [Dependency maintenance](docs/dependencies.md)
- [Media intake, queue limits and remaining containment work](docs/media.md)
- [Firecracker guest setup and local containment scope](docs/firecracker.md)
- [Durable media approval and operator publication](docs/media-approval.md)
- [Media approval verification](docs/verification-media-approval.md)
- [Staff WebAuthn setup, enrollment and moderation](docs/staff.md)
- [Approved media HTTP reader and separate identity](docs/media-http.md)
- [Authenticated private HTTP intake](docs/media-intake.md)

[Thread archives](docs/thread-archives.md) document rollover, optional retention policy, read-only HTML/JSON access and migration 0009.
