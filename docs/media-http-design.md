# Approved media HTTP serving

The next slice completes the path from an approved output to bytes served by an
independent read-only application. It is part of the requested media-origin and
least-privilege architecture; it does not enable public intake or claim attachment
compatibility. The initial route is project-defined `/media/{opaque-id}.png`.

Use a separate `board-media-http` Axum binary with `MediaReader` and
`ApprovedFiles`. Reusing the public app would give the reader unnecessary content
authority. A generic static server would bypass durable approval. The dedicated
reader instead checks the restricted approval view and bounded file length/hash
for every GET and HEAD, including conditional requests. It does not decode media.

Serve only host-encoded PNG, with a generated filename, `nosniff`, a restrictive
CSP, no cookies, and cross-origin image embedding. Successful responses use
`public, no-cache, must-revalidate`; errors use `no-store`. A matching ETag gives
304 only after approval and file verification. Range requests may receive the
complete bounded representation. No original uploads, directory listing, writes,
or user-selected filesystem paths are exposed. Request query strings and
noncanonical object paths are rejected.

Configuration requires explicit development mode, the existing reader database
login, absolute approved-storage path, distinct loopback origins and a matching
nonzero loopback listener. Production and public-media enablement remain rejected.
The binary rejects inherited application/writer credentials before connecting.
The HTTP process holds 16 request/response permits, retaining them through emitted
data, and four blocking-file permits retained by the blocking task even if its
request is cancelled. A ten-second handler deadline does not promise cancellation
of a stalled filesystem or a socket-write deadline. External unit limits remain
part of qualification. Health is process-only; readiness checks the approval view
and storage directory without publishing a probe object.

Publication defaults stay owner-only. An explicit `MEDIA_GROUP_READ=true` operator
setting enables a Unix shared publication root, preprovisioned with mode 02750
and a nonroot reader group. New bounded PNG output files become 0640 before
publication, independently of umask. Existing private files require a deliberate
operator migration; the runtime does not recursively change permissions. The
reader can physically read validated pending output as well as approved output,
but HTTP requires approval. It cannot change files, approval state, queue data,
content, staff identity, quarantine, or coordinator credentials. Trusted publisher
ownership and protected ancestor directories remain prerequisites.

Qualification uses synthetic fixtures in the owned disposable database and Linux
host: a real separately identified reader service, healthy controls for allowed
reads and denied writes/private reads, actual HTTP bytes/cache/error behavior,
approval removal, and cleanup. Existing VM dispatch tests establish processing
separately; the new service test must also serve an output produced by the real
dispatch exercise. Production DNS/TLS/network/backup policy, resource saturation,
power loss, and independent security review remain open.

The implementation reuses locked Axum 0.8.9 and Tokio 1.53.1. Axum's
[GET/HEAD behavior](https://docs.rs/axum/0.8.9/axum/routing/method_routing/struct.MethodRouter.html)
and Tokio's [blocking task cancellation limits](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html)
were checked against official API documentation. Cache decisions follow
[RFC 9111](https://www.rfc-editor.org/rfc/rfc9111.html).
