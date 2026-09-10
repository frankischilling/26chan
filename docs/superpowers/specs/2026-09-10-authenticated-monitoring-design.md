# Authenticated monitoring links

Prompt sections 6, 9 and 11 require authenticated service requests and actual
negative/positive evidence. Existing Rust scrapes require independent Bearer
tokens; Prometheus-to-Alertmanager and Alertmanager-to-webhook examples do not.
Add a usable authenticated profile and qualify it with the pinned real binaries.

## Choice and authority

Use native verified HTTPS and independent request credentials. Prometheus sends
HTTP Basic credentials to Alertmanager; Alertmanager sends a separate Bearer token
to the receiver. Protect Prometheus's own API with HTTPS and an independent
operator Basic credential. Alertmanager also has a separate operator credential.
Native Basic auth is broad API authority, including Alertmanager silences; this
does not implement per-route roles. Monitoring operators and the ingest principal
are trusted within that monitoring API boundary, never within application SQL or
deployment authority. Do not invent a proxy to imply finer authorization.

Compared with certificate-only authentication, request credentials permit testing
revocation on an existing connection. Native certificate configuration is reloaded
on new TLS connections, so CA/SAN replacement alone is not immediate revocation.
The receiver used for qualification is an owned HTTPS test endpoint, not a
production notification integration. Production receiver/account, OS identities,
network enforcement and operational credential ownership remain prerequisites.

## Profile renderer

`scripts/monitoring/auth_profile.py` is an operator library/CLI, not a service.
`render(manifest: dict, output: pathlib.Path) -> dict[str, pathlib.Path]` validates
everything before creating a new output directory; refuses an existing output.
The CLI takes `--manifest PATH --output PATH`. It never contacts a URL or prints
credentials/input values. Its only additional dependency is pyca bcrypt 5.0.0,
hash-pinned operator/test wheels; Rust dependencies do not change.

The strict JSON manifest has exactly these keys:

```text
prometheus: {listen, server_name, ca_file, cert_file, key_file, operator_password_file}
alertmanager: {listen, server_name, ca_file, cert_file, key_file,
               ingest_password_file, operator_password_file}
receiver: {url, ca_file, token_file}
scrapes: [{job, target, token_file}, ...]
rules_file: absolute path
```

All names/paths are strings. `listen` and scrape `target` are nonzero numeric
loopback sockets, IPv4 or bracketed IPv6. Listeners differ. `server_name` is an
explicit DNS name or IP without wildcards, credentials, port or control characters.
Receiver URL is HTTPS with hostname and explicit path, no credentials, query or
fragment. It may describe a future operator receiver; the renderer never sends.
Scrapes contain 1..4 unique closed jobs: board-public, board-staff, board-media,
board-monitor. Existing HTTP exporter tokens still authenticate these same-host
loopback scrapes. No cross-host plaintext scrape profile is introduced.

Each path is absolute/canonical and points to a regular non-symlink file. Secret
and private-key files deny group/other permissions on POSIX. Passwords/tokens are
64 lowercase hex bytes with an optional final newline; all credentials, including
scrape tokens, must differ. Bound manifest to 64KiB, keys/certificates to128KiB,
rules to256KiB. Reject duplicate JSON keys, unknown fields, wrong types, empty
scrapes, reused credentials, insecure endpoints and missing files. Native tools
subsequently check certificate/key/config validity. Bcrypt uses cost12.

Write JSON (valid YAML) with mode0600 in an operator-owned mode0700 output directory:
`prometheus.yml`, `prometheus-web.yml`, `alertmanager.yml`, `alertmanager-web.yml`.
Return keys `prometheus`, `prometheus_web`, `alertmanager`, `alertmanager_web`.
The Prometheus alertmanager target uses scheme https, verified ca_file/server_name,
TLS13 minimum, username prometheus and its password_file. Both server web configs
use TLS13, certificate/key files and nonempty basic_auth_users (operator on
Prometheus; prometheus/operator on Alertmanager). Alertmanager receiver uses its
HTTPS URL, verified TLS13 CA and Bearer credentials_file. Explicitly disable HTTP
redirect following on authenticated outbound clients. No insecure_skip_verify.
Rules, scrape intervals and alert routing otherwise retain the existing examples.
Gossip stays disabled through the documented `--cluster.listen-address=` command.

Native empty basic_auth_users intentionally disables auth: never generate it.
Unreadable native policy returns500; a malformed or empty deployment replacement
is not an approved rotation procedure. Document staging and native validation of
complete replacement files before atomic replacement. A trusted operator can
change policy; configuration validation does not defend against that operator.

## Real qualification

Use real board-observe http_fixture, Prometheus3.14.0, Alertmanager0.34.0 and an
owned HTTPS webhook. Generate synthetic test CAs/leaves with existing OpenSSL;
server names are verified, never skip verification. Use independently generated
credentials through private files and minimal child environments. Keep bounded
response/request sizes, connection timeouts, subprocess waits and notifications.

Prove missing/wrong Basic credentials fail on both APIs while an allowed request
works. Revoke the ingest password in native policy and prove its previously
working persistent connection is denied; unreadable policy must return500;
restore and prove recovery. Receiver policy is read per request and rejects
missing/wrong/expired/revoked tokens; unavailable policy returns503. Verify wrong
CA, wrong hostname and expired server certificates reject TLS while correct peers
work. These are authentication checks against owned local endpoints, not probes
of external systems. Native Basic credentials have operator-managed rotation, not
a built-in expiration field; do not describe them otherwise.

Drive actual HTTP failures through the Rust fixture and require the same alert's
firing/resolved webhook notifications after passage through both authenticated
hops. Verify failed credentials prevent delivery and corrected credentials resume
it. Keep production rule expressions/thresholds unchanged except the existing
documented transport timing acceleration. On normal exit and Linux OS SIGTERM,
stop/reap all owned children and remove temporary private files; cleanup must not
escape to unrelated process groups or paths.

Keep the original plain examples as explicitly named loopback test fixtures for
existing tests; document the new generated profile as the candidate for deployment.
Run focused renderer/helper tests and actual transport locally when available,
full existing CI plus dedicated authenticated transport/SIGTERM in Linux CI.
Merge only the passing head, recording actual results and limitations.

Sources: [native HTTPS/authentication](https://prometheus.io/docs/alerting/latest/https/),
[Prometheus configuration](https://prometheus.io/docs/prometheus/latest/configuration/configuration/),
[Alertmanager configuration](https://prometheus.io/docs/alerting/latest/configuration/),
[pinned request handler](https://github.com/prometheus/exporter-toolkit/blob/v0.17.1/web/handler.go),
[pinned TLS reload](https://github.com/prometheus/exporter-toolkit/blob/v0.17.1/web/tls_config.go),
[bcrypt5.0.0](https://pypi.org/project/bcrypt/5.0.0/).
