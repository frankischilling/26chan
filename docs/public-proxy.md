# Verified public proxy identity

Public rate limits, OP markup and OP self-bump rules use one canonical client
address. Linux proxy mode obtains the connecting process UID from the kernel,
then accepts exactly one `X-Board-Client-IP` header from the configured UID.
The value is a bounded IPv4 or IPv6 literal; IPv4-mapped IPv6 becomes IPv4.
Wrong or unavailable peer credentials return 403. Missing, duplicate, malformed
or oversized identity headers return 400 before handler admission or storage.
Neither form fields nor pre-existing request extensions override this identity.

Direct TCP development serving still uses the socket address and ignores every
forwarding header, including `X-Board-Client-IP`. Public production startup now
requires the Linux proxy settings. This changes deployment requirements without
changing the source's address-matching rules. It prevents the previous documented
TCP proxy arrangement from treating every client as the same OP and rate bucket.

## Configuration and permissions

Set both `PUBLIC_PROXY_SOCKET` and `PUBLIC_PROXY_UID`. The socket path must be
absolute, at most 100 bytes, and contain no traversal, repeated separators,
control characters or symlinked parent path. The numeric UID must match the
dedicated Nginx worker account. UID zero is rejected in production. Windows
rejects proxy mode; its direct loopback development listener remains available.
`BIND_ADDR` is validated but no public TCP listener opens in proxy mode.

Create a dedicated `board-edge` group for the public process and proxy worker.
The [candidate public unit](../deploy/public.service) runs as `board-public`
with that group and asks systemd to create `/run/paperboard-public` with mode
0750. The process checks parent ownership and rejects group/world-writable
directories. Its socket has mode 0660. Group membership permits connection;
the kernel UID check independently restricts which process can supply identity.
The proxy user must have no database, staff, decoder or deployment credentials.
Do not share its UID with unrelated applications.

Startup refuses existing sockets, regular files and symlinks. Shutdown drains
requests before removing the same socket inode it created. The service's
`RuntimeDirectoryPreserve=no` also gives systemd responsibility for cleaning
the runtime directory when the service stops, including failed service runs.
For manual execution after SIGKILL, an operator must inspect and remove only
the stale owned socket before restarting. The application never silently
unlinks an unknown path. Failure of another listener or startup dependency
releases the newly created public socket.

## HTTPS edge

Include [the Nginx server block](../deploy/public-proxy.nginx.conf) inside an
operator-owned `http` block, supplying real certificates, hostname and paths.
Set Nginx's dedicated worker user and numeric `PUBLIC_PROXY_UID` consistently.
The block overwrites the client identity header with `$remote_addr` and removes
the common forwarded-address headers. It forwards unchanged public paths to
the Unix socket, bounds body/header/inactivity timeouts, and disables upstream
retries. Its 256 KiB body limit matches text posting; production media stays off.
Access logs are disabled; the restricted error log still needs operator review.

Do not enable Nginx real-IP rewriting or PROXY protocol on this edge. A CDN,
load balancer, remote proxy or additional hop requires separate authenticated
identity handling and qualification. A trusted proxy compromise can assert
any client address; IP identity is not staff or account authentication.

The optional JSON API retains its separate TCP listener and read-only routes.
Give its HTTPS hostname a separate server block forwarding only to
`API_BIND_ADDR`. Staff and media retain their separate ingress and credentials.
The public block does not expose them. Firewall, certificate renewal, DNS,
service-account provisioning and host-level resource limits require the actual
operator environment; this candidate is not a production deployment claim.

Nginx's [proxy module documentation](https://nginx.org/en/docs/http/ngx_http_proxy_module.html)
defines the socket upstream and header replacement directives. systemd's
[execution documentation](https://github.com/systemd/systemd/blob/main/man/systemd.exec.xml)
defines runtime-directory ownership and cleanup.

## Qualification

Unit tests cover explicit configuration, canonical rate buckets, overwritten
request context and rejection of untrusted header values. Linux tests open real
Unix connections to verify kernel UID acceptance/rejection, duplicate/missing
headers, permissions, existing paths and cleanup. The existing paired-listener
test checks graceful draining.

`cargo test -p board-public --all-features --test proxy_https --locked` requires
an owned migrated PostgreSQL fixture, Nginx, OpenSSL and Python on Linux. It
starts the actual public binary and renders the candidate Nginx block with an
ephemeral certificate verified by the client. Distinct source addresses send
forged identity headers. The test checks separate write limits, saved OP markup,
address-only own-reply records, password ownership, no public TCP listener,
read-only API routing and socket cleanup. It uses only synthetic posts and
removes its fixture. The regular Linux workflow installs Nginx and runs this
test without a skip flag. Hosted results remain required for this change.

Rollback requires an explicit deployment decision: an older public binary has
no verified socket transport and loses per-client identity behind a TCP proxy.
Do not switch the edge back to that arrangement while claiming source-equivalent
OP behavior or individual rate limits. No database migration is needed here.
