# Approved media HTTP implementation plan

Goal: serve approved PNG bytes through a separately privileged reader and verify
the actual HTTP and storage boundary. Spec: [design](media-http-design.md).
Use the existing locked Rust/Axum/SQLx stack, safe first-party Rust, no new registry
dependencies, real disposable data, and existing Git identity. Implementation is
in the authorized current checkout on `feature/media-http`.

## 1. Reader and publication configuration

- [ ] Add subprocess configuration cases in `crates/config/tests/media_http.rs`
  for the valid exact reader environment and wrong/missing mode, credentials,
  path, origins and listener. Run `cargo test -p board-config --locked` and
  observe the missing-interface failure before implementation.
- [ ] Add `MediaHttpSettings::from_env()` in `crates/config/src/media_http.rs`,
  reexport it, and add `MediaAdminSettings::group_read`. Preserve current CLI
  defaults and configuration denials.
- [ ] Add Unix storage tests for private mode, rejected unprovisioned shared
  directories and newly installed shared files; then implement
  `PublicationStore::new_group_readable(root, quarantine)` and explicit operator
  selection in `media-publish`. Run `cargo test -p board-media --locked`.

## 2. Real HTTP read path

- [ ] Add workspace crate `apps/media-http`, tests and empty router interface;
  record a failed real approval/HTTP expectation before runtime implementation.
- [ ] Implement `AppState::new(reader, files)` and `router(state)` with GET/HEAD
  `/media/{id}.png`, `/healthz`, `/readyz`, safe errors/security headers, request
  and blocking-read permits, and conditional revalidation after checked reads.
  Add `MediaReader::ready()` for a read-only approval-view probe.
- [ ] Test actual pending/approved/corrupt/missing/removed approval states,
  200/304/HEAD, unsupported routes/methods, safe headers, held response admission,
  database failure, and retained blocking permits. Use real owned fixtures and
  `cargo test -p board-media-http --all-features --locked`.
- [ ] Add the executable using typed configuration and graceful shutdown. Build
  and exercise real loopback HTTP, including cross-origin image display in the
  pinned browser, with fixture cleanup.

## 3. Deployed test reader and integration

- [ ] Add `deploy/media-http.service` and an owned qualification test under
  `tests/media/`, with a distinct reader OS identity and protected environment.
  Run it against real approved output from dispatch, with positive controls for
  filesystem/database reads and denied writes/private paths. Verify actual unit
  identity, limits, HTTP behavior, and cleanup; keep production mode rejected.
- [ ] Wire the native test into CI after dispatch qualification. Update README,
  architecture, compatibility, operations/readiness and a verification record
  with exact commands, failures, platform limits, and remaining attachment scope.
- [ ] Run formatting, warnings-denied workspace Clippy, locked builds/tests,
  public/staff/browser checks, advisory checks and actual native qualification.
  Obtain source review, fix findings, push a focused PR referencing the issue,
  inspect final-head CI, and merge after passing checks as authorized.
