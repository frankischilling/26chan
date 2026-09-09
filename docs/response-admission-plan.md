# Response admission lifetime

The public/API and staff middleware currently release their concurrency permits when handlers return. Response bodies and data frames can outlive those handlers. Extend the existing request-admission policy through the lifetime of that response data, without changing page content, routes, body-size limits or status/header contracts.

## Constraints and design

- Keep the public/API shared capacity at 32 and staff capacity at 16. Keep overload responses at public/API 503 and staff 429; preserve API CORS/error normalization and staff private cache/security headers.
- Acquire an owned semaphore permit before existing validation/handler work. Keep it in response extensions through middleware that can replace a body, especially API CORS error normalization and HEAD/OPTIONS handling. Finalize the lease at an outermost body middleware layer.
- Put the small reusable implementation in `crates/http` (`board-http`). Export `hold_permit(Response, OwnedSemaphorePermit) -> Response` and the Axum middleware `retain_response_body(Request, Next) -> Response`. Use a private shared permit owner for the body and each emitted nonempty data frame. `Bytes::from_owner` keeps that owner with clones and slices without copying the payload.
- Release capacity when an unconsumed response is dropped, an empty response is finalized, or the completed/failed body and its previously emitted data are released. Pending reads retain admission. Preserve frame data, trailers, size hints and errors; cancellation must free permits. The private extension must not remain in the final response after ownership transfers to the body.
- Reuse locked `bytes` 1.12.1 and `http-body` 1.1.0; adding direct declarations and the local crate must not update existing registry versions. Keep first-party Rust safe. No database migration or screenshot-baseline changes.
- This bounds admitted handlers and retained application response data by count. It is not an aggregate byte/memory ceiling, a socket connection cap, a client receipt acknowledgement or a response-write deadline. Small overload responses, kernel buffers and copies made by downstream consumers remain outside that count. Slow clients, aggregate response budgets, external limits and proxy qualification remain explicit launch work.
- Media enablement stays rejected. No worker or deployed containment claim is part of this slice.

## Work and verification

1. Run failing real-router regressions showing public/API admit a 33rd request with 32 completed response bodies retained and staff admit a 17th with 16 retained.
2. Implement/test the shared ownership utility using the test-driven-development and subagent-driven-development skills. The delegated core task owns only `crates/http/**`, workspace `Cargo.toml` and `Cargo.lock`; root owns app wiring/tests/docs. Cover data held after body drop, clones/slices, pending bodies, error/EOF/trailers, empty responses and body replacement through middleware. Coordinate Cargo builds and lockfile changes.
3. Wire public/API and staff paths, prove admission stays occupied while actual data frames are held and recovers after drop, verify public/API sharing and error/HEAD/OPTIONS transformations, and keep existing handler-cancellation tests. Use bounded harmless fixtures; no high-volume load or third-party targets.
4. Run fmt, clippy, locked build, all workspace/database tests, public/staff browser suites with unchanged baselines, dependency audits, historical migration exercises, restore and actionlint. Review the whole change, document actual limits and results, commit with the configured identity and publish a draft PR. Check hosted Linux/Windows results before reporting completion.

The implementation uses the locked dependencies' source contracts for `Bytes::from_owner` and `http_body::Body`. No full compatibility or deployed resource-enforcement claim follows from these ownership tests.
