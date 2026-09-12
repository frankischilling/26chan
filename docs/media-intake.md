# Authenticated media intake

`board-media-intake` accepts private service uploads for development. It reserves
a job through the restricted `board_media_intake` PostgreSQL login, streams bytes
into quarantine and queues the completed input. It has no decoder, dispatch,
approval, content or staff authority. Public post attachment and production
enablement remain unfinished.

Provision the development login with `sudo bash scripts/dev-intake-db.sh` before
applying migrations. Use the example in `deploy/media-intake.env.example` in a
fresh service environment. Supply only the intake database login and independent
random service/metrics tokens. `board-media-intake --check-config` validates the
configuration without connecting to a database or opening listeners. Startup also
checks the real database role/grants and quarantine write/sync/unlink access.

All routes require exactly one `Authorization: Bearer <token>` header. Requests
with browser Origin or Fetch Metadata headers are rejected. Responses use private
no-store headers and fixed JSON errors. Tokens must never appear in URLs or logs.

| Request | Result |
|---|---|
| `POST /v1/reservations`, `application/json`, `{"filename":"display.png"}` | 201 with generated `id`, secret `capability` and `state: receiving` |
| `PUT /v1/uploads/{id}`, `application/octet-stream`, raw bytes | Requires `Upload-Capability`; 202 with `state: queued` and actual `input_bytes` |
| `GET /v1/uploads/{id}` | Requires `Upload-Capability`; returns bounded status and `output_id` only after approval |
| `GET /healthz` | Authenticated process health |
| `GET /readyz` | Authenticated database-contract and quarantine availability check |

Save the reservation capability before sending bytes. A claim permits one upload
only. Duplicate claims return 409; wrong, missing or expired capabilities return
404. Unavailable database/storage state returns 503. JSON is limited to 1 KiB and
uploads to 8 MiB, enforced while streaming with one byte of lookahead. Intake uses
[Tokio's pinned StreamReader adapter](https://docs.rs/tokio-util/0.7.19/tokio_util/io/struct.StreamReader.html)
to feed the existing quarantine writer. Empty/broken transfers fail; oversized
transfers return 413. A transfer has 15 seconds and a handler has 20 seconds.
Eight admitted responses and four concurrent upload bodies bound application
work. Request admission remains occupied while emitted response data is retained.

Receiving jobs expire after five minutes; queued jobs expire after one hour.
Ordinary transfer failure removes the partial file and attempts to fail its own
claim. Cancellation or unavailable SQL can leave a claimed receiving row for
expiration and operator reconciliation. A complete private input remains after an
uncertain final database result because queuing may have committed. Inspect status
and reconcile through existing fenced cleanup. Do not retry the upload or remove
possibly queued bytes. Failed input is never published as a fallback.

The candidate unit uses a distinct UID, an exclusively writable quarantine root,
no capabilities, a read-only system, a 256 MiB memory ceiling and 32-task ceiling.
Its listener and optional metrics endpoint bind only to loopback. Metrics use the
fixed `intake` listener label. SIGINT/SIGTERM drain admitted requests and close
both listeners. These are development constraints; the unit does not establish
production network denial or external storage quotas.

The owned native qualification runs the candidate service as its actual UID,
uploads a synthetic PNG, uses the existing authenticated Firecracker gateway,
approves the result and reads it through the separate HTTP reader. The development
operator runs dispatch with its existing filesystem authority to read the
intake-owned quarantine. This does not qualify a deployed coordinator/storage
design. It also checks healthy permission controls, real chunked overflow,
disconnect/deadline cleanup, service shutdown and interruption cleanup:

```bash
# After the development database, workspace binaries and disposable VM profile exist:
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-ci/decode.json \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-ci/probe.json \
  bash scripts/test-media-intake.sh
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  MEDIA_VM_TEST_CONFIG=/tmp/26chan-media-ci/decode.json \
  MEDIA_VM_PROBE_CONFIG=/tmp/26chan-media-ci/probe.json \
  bash scripts/test-media-intake.sh --interrupt
```

See [verification](verification-media-intake.md) for actual outcomes. Production
service authentication, network and storage policies, post attachment, additional
formats, original-file retention and independent review remain required.
