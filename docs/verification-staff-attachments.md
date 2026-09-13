# Staff attachment verification

This checkpoint covers the staff portion of [issue #48](https://github.com/frankischilling/26chan/issues/48) on `feature/post-media-attachments`, [PR #49](https://github.com/frankischilling/26chan/pull/49). It implements a staff-only attachment display view and audited file removal. Original private moderation behavior is unknown; this is project-defined behavior, not verified original-site parity.

## Local checks

The owned Windows PostgreSQL 16.15 database was migrated through 0017 on September 13, 2026. These commands passed from the repository root:

```powershell
$db = Get-Content .local/intake-postgres/current-database.json -Raw | ConvertFrom-Json
. $db.env_path
$env:OPENSSL_SRC_PERL = 'C:\Users\imike\4chan-rewrite\.local\strawberry-perl\perl\bin\perl.exe'
cargo check -p board-staff --all-targets --all-features --offline --jobs 1
cargo run -p board-store --bin board-migrate --locked --jobs 1
cargo test -p board-staff --all-features --locked --jobs 1
cargo clippy -p board-staff --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo build -p board-staff -p board-public -p board-media-http --bins --examples --locked --jobs 1
npx playwright test --config playwright.staff.config.js
npx playwright test --config playwright.staff.config.js --repeat-each=2
python scripts/test-attachment-restore.py
cargo fmt --all -- --check
node --check tests/browser/staff.spec.js
```

All 28 staff tests passed, including the new attachment test. The browser scenario passed once, then passed both repetitions after the final build. The database fixture uses actual `board_staff`, `board_auth`, `board_public` and `board_media_read` connections. It verifies exact display-view columns and read-only grants, escaped filenames, bounded thumbnail attributes, spoiler non-fetching, recent authentication, CSRF, object scoping, absent/expired/revoked sessions, rejected roles and content preservation. Two simultaneous removals produce one success, one 404 and one audit entry. A real audit-table lock holds the mutation before commit; cancellation rolls back the attachment deletion and audit append. Removed posts retain review metadata without reopening their files.

The Chromium test uses a virtual WebAuthn authenticator and the actual staff, public and media-reader binaries. It verifies an image's natural dimensions, absence of staff cookies and referrers on media requests, explicit spoiler opening without an opener, JavaScript-disabled file removal after login, public JSON/ETag changes and full/thumbnail 404s with old validators. The reader receives only its own database credential. Generated accounts, records, media files and invitation directories are removed after the test; the reader and application processes are stopped.

The first browser run passed the removal and reader-denial assertions, then failed because the harness requested headers from a closed image tab. Capturing headers at request time fixed the harness; the complete scenario then passed. An earlier compile started before the example's new development dependency was loaded and rejected the import. A fresh offline resolution added only the existing workspace `board-media` development dependency to the lockfile; no registry package changed. The documented local Perl configuration resolved the previous turn's missing-Perl staff build limitation.

The populated restore exercise passed again after migration 0017, including missing-file/corrupt-file controls, unchanged-source checks and cleanup. The previous restore head `36672c8` also passed [complete PR CI](https://github.com/frankischilling/26chan/actions/runs/34732733196). Staff head `b6b7caa` subsequently passed [complete PR CI](https://github.com/frankischilling/26chan/actions/runs/34733748163), including its Linux/native and Windows checks.

## Scope and operating requirements

The browser fixture encodes fixed trusted pixels and inserts synthetic approval records with migration authority. It does not exercise upload intake, guest processing or approval authorization. Those tests remain separate. All local processes run under the test operator's Windows identity; this is not evidence of deployed filesystem, network or service-identity isolation. Linux-specific shutdown checks do not run on Windows. No physical authenticator, independent security review or original visual baseline is claimed.

Migration 0017 must precede the staff binary. Staff readiness checks its new display view. No new login, raw attachment grant, upload capability or publication permission is added. Staff and media require different hostnames even in development, since different ports do not isolate cookies. Production still requires a distinct registrable media domain and the deployment prerequisites in [staff operation](staff.md).

File removal revokes reader access and records an audit entry atomically. It does not erase previously downloaded copies or backups. Physical cleanup uses the canonical-store [retention procedure](post-attachments.md#output-retention-and-cleanup). Staff cannot reopen removed/expired files or bypass reader approval. The subsequent [legacy normalized-manifest upgrade](legacy-media-upgrade.md) has separate local evidence and awaits native CI. Production retention scheduling, full visual/reference compatibility and launch qualification remain unfinished.
