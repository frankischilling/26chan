# Authenticated media intake

Implement [issue #46](https://github.com/frankischilling/26chan/issues/46) as the
next prerequisite for the upload-to-post flow. The existing operator-only
pipeline already quarantines input, dispatches through an authenticated gateway
to Firecracker, validates stopped output, grants durable approval and serves it
through a separate reader. Add real service intake to that path. Public post
attachment remains required work; this checkpoint does not enable public uploads
or production mode, or redefine the finished imageboard as operator-only.

## Authority

Add `board_media_intake` as a NOLOGIN runtime role until an operator explicitly
provisions its login. It has no table privileges, role membership, schema-create
privilege, application/staff/deployment access, processing claim authority or
asset-approval authority. It receives only USAGE on a new `media_intake` schema
and EXECUTE on five narrow functions plus readiness. Add
`board_media_intake_owner` as a NOLOGIN function owner with only the underlying
queue/policy/handle privileges those functions require. The migration role may
SET ROLE to that owner for installation; no runtime may do so.

Functions are SECURITY DEFINER, use a fixed `search_path = pg_catalog, pg_temp`,
fully qualify application objects, and revoke PUBLIC EXECUTE in the same
migration transaction. Do not let the runtime create overloads, replace functions
or change ownership. This follows the [PostgreSQL 16 function guidance](https://www.postgresql.org/docs/16/sql-createfunction.html).

The new Axum process holds only its intake database login, a service bearer
credential, its private quarantine root and optional private metrics credential.
It cannot decode, dispatch, validate media output, promote or approve files.
A compromised intake process can fill its admitted queue and access its own
quarantine root; it cannot obtain content/staff/deployment data or processing
leases through the granted database interface. The existing per-job guest
boundary and its absent credentials/network/storage access remain unchanged.

## Database protocol

Migration `0011_media_intake.sql` adds `media_intake.handles`: job_id references
`media.jobs(id)` ON DELETE CASCADE; capability_hash is a 32-byte SHA-256 digest;
upload_started_at is nullable. Do not store the raw capability. Generate the
capability from two independent random PostgreSQL UUIDs, concatenate their
hyphen-free text (64 lowercase hex characters), return it only at reservation,
and hash its UTF-8 bytes with PostgreSQL's built-in SHA-256 function.

The functions are:

```sql
media_intake.reserve(filename text) RETURNS TABLE(id text, capability text)
media_intake.begin_upload(id text, capability text) RETURNS void
media_intake.finish_upload(id text, capability text, input_bytes bigint) RETURNS void
media_intake.abort_upload(id text, capability text) RETURNS void
media_intake.status(id text, capability text)
  RETURNS TABLE(id text, state text, input_bytes bigint, output_id text)
media_intake.ready() RETURNS boolean
```

Reserve validates a 1..255 UTF-8-byte filename without control characters,
locks the existing singleton capacity row, counts the same unfinished states
as `MediaQueue::reserve`, and atomically creates a receiving job and capability
hash. The existing five-minute receiving deadline applies. Existing operator
jobs have no handle and are inaccessible through these functions.

Per-object calls validate exact lowercase-hex ID/capability lengths and verify
the stored digest. Missing/wrong capability or unknown handle yields the same
not-found error (`P0002`). An expired receiving/queued job also denies access;
failed/published status remains available until existing terminal cleanup removes
the job. Unavailable database state never authorizes an operation.

Mutation lock order is job row then handle row. `begin_upload` is an exclusive
claim: only an unexpired receiving job with no prior upload_started_at succeeds.
Duplicate/concurrent attempts return conflict (`P0001`) before filesystem work.
`finish_upload` requires that claim and an unexpired receiving job, enforces
1..8,388,608 bytes, and performs the existing receiving-to-queued transition with
its one-hour deadline. It cannot modify attempts, leases or approvals.
`abort_upload` may mark only the same receiving job intake_failed; it cannot
change processing/published/failed jobs. Cancelled claims are not reusable.

Status maps a claimed receiving job to `uploading`, otherwise returns its actual
state. It exposes a generated output_id only from an approved asset associated
with this job, in the same database snapshot. It never returns a filename,
input bytes, lease token, capability hash, filesystem path or arbitrary worker
metadata. Existing queue/asset retention and cleanup remain authoritative;
handle cleanup cascades when a terminal job is deleted.

## Rust store interface

`crates/store/src/media_intake.rs` exports the following types, with private
connection state and no Debug implementation revealing secrets:

```rust
#[derive(Clone)]
pub struct IntakeStore { /* private PgPool */ }
pub struct IntakeReservation { pub id: String, pub capability: String }
pub struct IntakeStatus {
    pub id: String, pub state: String,
    pub input_bytes: Option<i64>, pub output_id: Option<String>,
}
// All methods below return Result<_, StoreError>.
// async connect(url: &str) -> IntakeStore
// async reserve(&self, filename: &str) -> IntakeReservation
// async begin_upload(&self, id: &str, capability: &str) -> ()
// async finish_upload(&self, id: &str, capability: &str, bytes: u64) -> ()
// async abort_upload(&self, id: &str, capability: &str) -> ()
// async status(&self, id: &str, capability: &str) -> IntakeStatus
// async ready(&self) -> ()
// async close(&self) -> ()
```

Use bound SQLx parameters only. Map P0002 to StoreError::NotFound, P0001 to
Conflict with static copy, 22023 to Invalid with static copy, and other database
failures to Database. Startup checks the actual role, prohibited flags,
memberships, database/schema ownership, forbidden schema/table privileges,
function ownership/definer/search-path configuration and execute grants. Also
check the owner remains NOLOGIN without elevated role flags or membership.

## HTTP contract

`apps/media-intake` builds `board-media-intake`. Configuration requires
`MEDIA_INTAKE_MODE=development`, a nonzero loopback-only `MEDIA_INTAKE_BIND`,
`INTAKE_DATABASE_URL` naming board_media_intake on loopback with no query or
fragment, an absolute `MEDIA_QUARANTINE_DIR`, and a 64-character lowercase-hex
`MEDIA_INTAKE_TOKEN`. Reject production/unknown mode and unrelated database,
dispatch or deployment credentials. Existing runtimes must reject the new intake
database credential too. Do not add it to public serving.

All routes require exactly one `Authorization: Bearer <service token>` header.
Per-object operations additionally require exactly one `Upload-Capability`
header with 64 lowercase hex characters. No tokens appear in URLs. Reject
browser-origin requests and provide no CORS allowance. Return fixed JSON errors
and private/no-store, nosniff and frame-denial headers; never log request bodies,
filenames, URLs, capabilities or credentials.

| Route | Behavior |
|---|---|
| POST /v1/reservations | Strict JSON `{ "filename": "display.png" }`, at most 1 KiB; 201 `{id,capability,state:"receiving"}` |
| PUT /v1/uploads/{id} | application/octet-stream, capability required; exclusive claim before opening a generated quarantine path; 202 `{id,state:"queued",input_bytes}` after real streaming and database finalization |
| GET /v1/uploads/{id} | Capability-protected bounded status from the store, with optional approved output_id; no files |
| GET /healthz | Authenticated process health |
| GET /readyz | Authenticated store-contract and quarantine availability check; 503 on unavailable state |

Use the existing `Quarantine::receive` so the input limit remains 8 MiB plus
one lookahead byte, without collecting the full body. Adapt body data frames
through pinned `tokio-util = 0.7.19`, feature `io`, using
[StreamReader](https://docs.rs/tokio-util/0.7.19/tokio_util/io/struct.StreamReader.html).
Do not write a second HTTP parser or decoder. Use existing locked futures-util
and http-body-util for stream/error adaptation; add no HTTP client dependency.
The owned test client uses Python's standard-library HTTP client.

Enforce eight admitted requests with permits retained through response ownership,
at most four uploading bodies, a 15-second receive deadline, and a 20-second
overall handler deadline. Reject unavailable admission with 503/Retry-After.
Reject authentication before reading a body or reserving capacity. Malformed
input is 400/422, excess body bytes are 413, unsupported media type is 415,
wrong object capability is 404, duplicate/nonreceiving upload is 409, and database
failure is 503. Queue saturation is 503. Invalid Content-Length is never trusted
instead of the streaming limit; chunked/absent-length requests have the same cap.

Ordinary receive failure removes partial bytes and attempts an authenticated
abort. Cancellation can leave a claimed receiving row; its deadline and operator
cleanup reconcile it. After Quarantine finishes, any uncertain queue outcome
retains the complete private input for reconciliation. Never delete a possibly
queued input, reuse a cancelled claim or publish raw bytes. The two-phase API
lets a caller retain its ID/capability before streaming and inspect an uncertain
completion without uploading twice. No automatic client retry is added.

Serve private fixed-label metrics through the existing observer endpoint with
an `intake` listener label. Shutdown handles SIGTERM and SIGINT and closes both
listeners. Candidate service settings constrain UID/filesystem/capabilities and
resources; production remains rejected. The local intake UID owns quarantine;
the current operator dispatcher reads it with existing operator authority. Do
not claim this qualifies a future deployed coordinator or shared storage policy.

## Verification and completion

Use synthetic data and benign bounded failures only. Actual database logins must
prove all denied authorities with healthy allowed controls. Test wrong/missing/
expired capabilities, unavailable grants, concurrent begin/finish, capacity,
duplicate completion, metadata rejection, old operator jobs, approved-only
status, cleanup cascade, migration upgrade and restored grants/data.

HTTP tests exercise real handlers, auth-before-intake, 1-KiB JSON limits,
streaming overflow without Content-Length, disconnect/deadline cleanup,
single-writer exclusion, unavailable storage, private headers and admission.
No test-only route or worker-output acceptance bypass goes into the application.

The owned Linux harness starts the actual intake candidate service with a
distinct identity, calls HTTP reserve/upload, invokes the existing authenticated
Firecracker dispatcher, checks status's approved output, and reads exact generated
PNG bytes through the separate reader. Prove protected file/database denials
from the intake context, normal and SIGTERM cleanup, and retain existing guest
denial/resource checks. Extend CI, role bootstrap, historical migration and
restore qualification. Passing portable tests do not replace native execution.

Do not merge until reviewed-head checks pass. Record exact commands, failed or
unrun checks, service identities and limitations. Post attachment, reference
parity, original downloads, additional formats, production deployment/containment,
power-loss/retention policy and independent review remain in the full goal.
