# Media intake verification

The database prerequisite is implemented on `feature/media-intake`. The HTTP
service and owned end-to-end qualification are in progress. Public uploads and
production media remain disabled. This record does not establish public post
attachment, reference parity, production containment, or independent deployment
review.

## Database checkpoint

Commits `82e71e14` and `36f9f800` add the narrow runtime and NOLOGIN function
owner, hashed reservation capabilities, exclusive receiving claims and scoped
status. Reservation requires Read Committed at the SQL boundary because its
capacity count needs a fresh snapshot after the singleton row lock. Unsupported
isolation is rejected before admission. Source review found that missing
precondition; the fix and covering tests passed scoped review.

On September 10, 2026, actual PostgreSQL 16.15 Windows tests used an independent,
loopback-only SCRAM cluster in a private workspace directory. The existing WSL
cluster was not changed. Local commands used Rust 1.94.0 through a private
RUSTUP_HOME and Cargo target, with `--jobs 1` and the existing portable Perl.

- `cargo run -p board-store --bin board-migrate --locked --jobs 1` applied all
  migrations using the real migration login. The changed 0011 was applied to a
  fresh database, without changing an applied checksum.
- `cargo test -p board-store --features database-tests --test media_intake --locked --jobs 1`
  passed both tests. The matrix uses real intake, migration and processing logins
  for capability, capacity, concurrency, expiry, transition, approval, cleanup,
  direct-authority denial and grant-drift checks. The unsupported-isolation
  test failed before the guard and passed after it; no capacity-race reproduction
  was used.
- Existing `media_queue` and `media_assets` integration suites passed all five
  tests. Store Clippy with warnings denied, scoped formatting and Bash syntax
  checks passed.
- A Windows adapter executed the exact historical Linux harness SQL blocks:
  baseline 0010 jobs/assets, explicit migration rollback, committed upgrade,
  unchanged rows, no fabricated handles, real intake operations, legacy-job and
  table denials, processing/approval and approved-only reader controls passed.
  The separate owned upgrade database was removed afterward.

The first store connection check failed with SQLSTATE 42501 because catalog
validation resolved protected relation names under the restricted caller. Using
catalog object IDs corrected that check; the same real-login tests then passed.
The exact Linux bootstrap and historical migration shell lifecycles remain a CI
requirement; Windows SQL execution does not replace them.

## Restore and configuration checkpoint

The existing restore command failed on `ALTER FUNCTION ... OWNER TO
board_media_intake_owner`: that owner intentionally lacks schema CREATE. Restore
now uses the already trusted bootstrap administrator and a single transaction to
preserve original ownership and ACLs. The root operator opens the private backup
and passes its stream to the peer-authenticated restore process. No additional
schema grant is left on the function owner. This follows PostgreSQL's
[ownership requirements](https://www.postgresql.org/docs/16/sql-alterfunction.html)
and [restore ownership behavior](https://www.postgresql.org/docs/16/app-pgrestore.html).

The equivalent native Windows dump/restore passed, followed by both intake tests
against the restored database. Its generated database was then removed. The
Linux peer-authenticated command and expanded handle-data restore checks still
need integrated qualification.

Existing public, processing, reader, staff and observer configuration rejects
the intake database credential. New process-isolated regressions failed before
the changes. Afterward, board-config passed healthy and string/non-Unicode denial
cases for four runtime roles, and the resource/maintenance observer tests passed.
The staff post-fix test could not run locally: its build and one idle retry failed
mapping Rust metadata with Windows OS1455, stating the paging file was too small.
It remains unverified until a successful native run. No global memory setting,
unrelated user process, compiler diagnostic or assertion was changed to bypass
that failure.

The CI checkpoint provisions the intake login before migration, sources its
private test environment, preserves the existing workspace/native/browser suites
and adds the historical migration command. Exact hosted outcomes will be recorded
after those runs finish. HTTP streaming, candidate service identities, VM dispatch,
approval/reader integration and normal/SIGTERM cleanup remain required before this
branch can be considered complete or merged.
