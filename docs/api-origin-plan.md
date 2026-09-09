# API origin implementation plan

The supplied rewrite prompt authorizes this continuation. The pinned API README describes CORS from board origins for GET, HEAD and OPTIONS. Exact cache-header exposure and preflight error responses are project decisions. No original source, live posts or media enter fixtures.

The public application will optionally bind a second, JSON-only listener. Both listeners share the public database pool, concurrency limits and process identity. This is an interface boundary, not a new privilege boundary. The existing HTML and posting listener remains available. The API listener permits browser reads only from the exact configured public origin, never credentialed CORS or writes. Media and staff authority remain separate.

## Task 1: API routes and CORS

Files: `apps/public/src/lib.rs`, a focused API HTTP module, and `apps/public/tests/api_cors.rs`.

- [x] Add failing request tests for allowed and denied origins, preflight methods/headers, bodyless HEAD/OPTIONS/304, cache variation, errors, unavailable storage and absent write/HTML/staff routes.
- [x] Implement `routers(pool: PgPool, origin: String, production: bool) -> (Router, Router)` with shared state. Preserve `router(...) -> Router` for existing consumers.
- [x] Serve the implemented JSON endpoint shapes and health/readiness on the API listener. Permit GET/HEAD and OPTIONS; allow only `if-none-match` and `if-modified-since` in preflight. Expose ETag and Last-Modified. Preserve Vary and add Origin, including denied-origin and no-Origin variants.
- [x] Run the focused suite and commit only this task's files through the installed Git workflow helper.

## Task 2: Configuration and real browser integration

Files: `crates/config/src/lib.rs`, public startup/main, `playwright.config.js`, browser tests, reference manifest, deployment example and documentation.

- [x] Add failing startup/configuration tests. Require API_ORIGIN and API_BIND_ADDR together; origin distinct from public/staff/media, HTTPS and DNS in production, loopback in development. Keep media's registrable-domain separation from API. Reject duplicate bind addresses before connecting to storage.
- [x] Bind both listeners before serving either. Share router state; shut down both listeners and close the pool. Existing one-listener startup remains supported.
- [x] Add browser checks using a controlled board-origin test document and the real API listener/database. Verify an allowed fetch and conditional fetch, rejected credentialed/unapproved-origin reads and rejected writes. Keep screenshot baselines unchanged.
- [x] Pin/hash the API README alongside existing reference metadata. Update I-010 and record remaining exceptions.
- [x] Run formatter, strict Clippy, locked workspace/database tests, public/staff browser tests and Windows screenshots. Review the combined diff, commit, push and open a draft PR.

## Decisions and limits

Use the current checkout on a new feature branch as the prompt requests. The installed workflow's optional worktree is unnecessary here because inventory found no user changes. The second listener is opt-in to preserve existing run instructions. Public and API origins use one explicit configured board origin, not the original service's domains. Changing this to multiple board origins later requires an explicit allowlist and tests.

Media qualification and reference collection remain separate open issues. `/dev/kvm` exists in WSL, but no maintained guest, qualified coordinator or deployed test policy is available. Public processing stays disabled. This checkpoint makes no 1:1 or production-readiness claim.
