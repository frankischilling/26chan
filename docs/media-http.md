# Approved media HTTP reader

`board-media-http` serves generated PNG output through the existing
`board_media_read` PostgreSQL login. Every GET and HEAD of
`/media/{32-lowercase-hex-id}.png` requires a current row in
`media.approved_assets` and matching bounded file bytes. This is a project-defined
route, not a claim about the pinned legacy media URL contract.

The service never reads quarantine, decodes uploads, accepts posts, changes
approval, or exposes original downloads. Approval removal makes subsequent
requests return 404. A request already authorized by its database read may finish
while an operator removes approval. There is no erasure guarantee for previously
downloaded bytes.

## Run locally

Prepare an approved output with the existing [dispatch and publication
workflow](media-dispatch.md). Use its generated output ID and object directory.
In a separate shell, give the reader only its own environment:

```bash
source .local/media-reader.env
unset DATABASE_URL TEST_PUBLIC_DATABASE_URL MIGRATION_DATABASE_URL MEDIA_DATABASE_URL STAFF_DATABASE_URL AUTH_DATABASE_URL
export APP_ENV=development
export MEDIA_ORIGIN=http://127.0.0.1:3002
export MEDIA_BIND_ADDR=127.0.0.1:3002
export MEDIA_APPROVED_DIR=/absolute/path/to/objects
cargo run -p board-media-http --locked
```

Open `http://127.0.0.1:3002/media/OUTPUT_ID.png`, substituting the actual approved
ID. `/healthz` checks process availability; `/readyz` reads the restricted view
and opens the storage directory. Readiness does not scan every output or promise
that a future file read will succeed. No response sets a cookie. Development
origins use separate loopback ports, which do not isolate cookies; responses
ignore cookies, and the browser test confirms identical returned bytes with a
synthetic cookie. Production requires a separate registrable media domain and
deployed HTTPS/network policy. This binary currently rejects production mode.

Configuration requires explicit origin, listener, absolute storage path and
development mode. The listener must match its loopback HTTP origin. Public,
staff and optional API origins must be distinct. Inherited public, writer,
migration or staff database credentials, a different reader login, remote
database URLs, and `MEDIA_ENABLED=true` are rejected before connection.

Successful responses use `image/png`, a generated inline filename, `nosniff`, a
restrictive sandbox CSP and `Cross-Origin-Resource-Policy: cross-origin` for image
embedding. There is no credentialed CORS or arbitrary active-content endpoint.
GET and HEAD support ETag revalidation after checking approval and actual bytes;
cache policy is `public, no-cache, must-revalidate`. Errors use `no-store` and no
ETag. Ranges receive the complete bounded representation with `Accept-Ranges:
none`. Queries, request bodies, noncanonical paths and inappropriate Host values
are rejected. These choices are explicit local behavior pending attachment and
legacy-media compatibility work.

## Separate Unix reader identity

The ordinary publication store remains owner-only. For a distinct reader OS
account, provision a dedicated store while processing is stopped. The example
assumes the existing coordinator account and a deliberately created nonlogin
reader account/group:

```bash
sudo install -d -o root -g root -m 0755 /var/lib/26chan-media-http
sudo install -d -o board-media-coordinator -g board-media-reader -m 2750 /var/lib/26chan-media-http/objects
```

Set `MEDIA_GROUP_READ=true` only in the coordinator's publication environment and
point dispatch at that object root. The publisher requires an existing 02750
directory with a nonroot group. It writes validated bytes privately, then sets
0640 before linking the completed PNG, independently of umask. Directory setgid
selects the reader group. The reader never joins the coordinator group. Shared
publication is Unix-only. Existing private files remain private; moving an
existing store requires a reviewed, ID-specific permission migration and byte
verification. The application does not recursively change existing permissions.

The reader can physically read validated pending output in this directory.
HTTP still checks approval, so a file's presence alone cannot publish it. The
coordinator owns the directory and files; the reader has no write permission.
Protect every ancestor against replacement by the reader or workers. Only a
trusted publisher may write this store, and one database must have one deliberate
output-root mapping. No shared quarantine or worker input directory is exposed.

`deploy/media-http.service` is the tested candidate unit. Install the binary and
a root-owned 0600 `/etc/paperboard/media-reader.env` at its configured paths before
using it. The environment file supplies only the reader database URL, explicit
development mode, origins/listener and approved directory. It has no migration,
writer, staff, cloud or deployment credentials. The unit sets a separate user and
group, empty capabilities, read-only filesystem policy, protected coordinator
paths, 256 MiB memory, one CPU quota, 32 tasks, 256 descriptors and a ten-second
stop deadline. Review all paths against the intended host before installation.

The application admits 16 handlers/retained responses and four blocking file
operations; each file is at most 5 MiB. Cancelling a handler does not free a
blocking-read slot until its work finishes. The ten-second handler deadline does
not cancel a blocked filesystem call or impose a socket-write deadline. The OS
unit limits and proxy connection/write limits remain necessary. Qualification
reads actual cgroup limits; it does not establish saturation performance or
total process memory from the application counts.

## Verification and remaining deployment work

The all-feature Rust suite includes a real browser test of the actual reader
binary. Install Node and the pinned Playwright browser as in the README before
running it. Native database/router checks can be run separately with
`cargo test -p board-media-http --features database-tests --locked`.

The owned Linux qualification uses actual temporary copies of the candidate
broker, gateway and reader units, real dispatch/Firecracker output, distinct OS
identities and existing disposable database roles:

```bash
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_TEST_CONFIG=/absolute/path/to/decode.json \
  MEDIA_VM_PROBE_CONFIG=/absolute/path/to/probe.json \
  bash scripts/test-media-dispatch.sh --http
```

Set `MEDIA_DISPATCH_BIN_DIR` when native binaries are outside `target/debug`.
The test verifies its rendered reader unit before starting it, checks HTTP
approval/hash/cache decisions and actual process permissions with healthy
controls, rejects production/inherited-credential startup, and removes its own
services, queue records and files. It creates or reuses a nonlogin
`board-media-reader` account and keeps that setup account afterward.

These results qualify only the recorded owned test profile. The reader can
contact loopback services; production egress/network policy is not qualified.
It can read ordinary world-readable host files allowed by the candidate mounts.
Production DNS/TLS/domain isolation, dedicated-host/storage policy, saturation,
power loss, backups, monitoring/alerts, independent review, public attachment,
legacy formats/thumbnails and original-download policy remain prerequisites.
Public intake and production enablement stay disabled. See [verification
results](verification-media-http.md) and [readiness](readiness.md).
