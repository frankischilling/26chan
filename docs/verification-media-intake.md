# Media intake verification

The database prerequisite and HTTP service are implemented on `feature/media-intake`.
Owned end-to-end qualification is in progress. Public uploads and
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

## HTTP checkpoint, September 12, 2026

The service now implements authenticated reservation, exclusive streaming upload,
capability status, health/readiness and fixed-label metrics. It uses the restricted
store and existing quarantine writer. Configuration rejects production, unrelated
credentials, nonloopback addresses and reused service/metrics tokens.

On the existing owned PostgreSQL 16.15 Windows cluster, the following passed:

- `cargo test -p board-media-intake --all-features --locked --jobs 1`: three
  middleware/metrics tests, two process-isolated configuration tests and the
  actual-login HTTP integration test. The integration covers strict JSON,
  missing/wrong authorization, real bytes, duplicates, absent-length overflow,
  broken/empty bodies, cancellation, the actual 15-second receive deadline,
  expiration, missing storage and retained complete input after database closure.
- `cargo test -p board-media-intake -p board-observe -p board-media --all-features --locked --jobs 1`:
  intake coverage plus 33 observer and 33 media tests passed.

The initial config-test build failed because the unfinished package had no binary.
The first middleware test needed an explicit response type for its panic sentinel.
The first scoped Clippy run rejected a large response error variant; the helper
now returns a status code. No diagnostics or assertions were disabled.

CI now includes the candidate intake unit and HTTP-to-Firecracker qualification,
plus a SIGTERM interruption cleanup run. Their hosted outcomes remain pending.
Portable/native Windows tests do not prove Linux service or VM containment.

Follow-up checks passed `cargo test -p board-media-intake --all-features --locked --jobs 1`
with four middleware/metrics tests, two configuration tests and the real-database
HTTP matrix. It now also proves duplicate valid service/capability headers are
denied, four pending bodies exhaust upload admission, status remains usable and
cancelling the bodies restores upload capacity. Scoped Clippy passed with warnings
denied. The two intake store, two queue and three asset tests also passed against
the owned PostgreSQL cluster.

The live-handle restore adapter `.local/intake-postgres/check-live-handle-restore.ps1`
passed on that cluster. It reserved and claimed a synthetic upload before dumping,
restored atomically under the bootstrap identity, matched the entire handle row
fingerprint, authenticated the retained capability, completed its restored claim
and rejected wrong capabilities/table/approval access with healthy migration-login
controls. The disposable restored database and source fixture were removed; the
private backup remains. The Linux restore script now exercises these same live
handle/claim checks in addition to its existing data and permission assertions.
Exact Linux shell lifecycle execution remains pending CI.
