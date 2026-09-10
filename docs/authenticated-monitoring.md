# Authenticated monitoring profile

The profile renderer adds verified HTTPS and independent request credentials to
Prometheus, Alertmanager and the notification link. It keeps Rust exporter
scrapes on authenticated same-host loopback HTTP. The original plain HTTP files
in `deploy/monitoring/` remain synthetic test fixtures. Use the generated profile
as the candidate for an owned deployment, subject to the prerequisites below.

| Connection | Credential | Authority |
|---|---|---|
| Prometheus to each Rust exporter | Independent Bearer token per exporter | Read that exporter's fixed metrics |
| Prometheus to Alertmanager | Basic user `prometheus`, independent password | Broad Alertmanager API access, including silences |
| Alertmanager to receiver | Independent Bearer token | Whatever the selected receiver authorizes; the test receiver accepts notifications only |
| Operator to Prometheus | Basic user `operator`, independent password | Broad Prometheus API access |
| Operator to Alertmanager | Basic user `operator`, separate password | Broad Alertmanager API access |

Native Basic authentication does not implement per-route roles. The ingest
principal must be trusted within Alertmanager's entire API boundary. These
credentials grant no application database, staff account, backup or deployment
authority. Keep service OS identities, private directories and configuration
ownership separate. Network policy must deny unrelated destinations and public
ingress to monitoring APIs. The generated profile does not deploy that policy.

## Prepare and render

Install the operator-only dependency into an ignored virtual environment. The
requirements file pins bcrypt 5.0.0 and official binary wheel hashes; it adds
nothing to the Rust application dependency graph. Linux x86_64 example:

```bash
python3 -m venv .local/monitor-auth-venv
.local/monitor-auth-venv/bin/python -m pip install --only-binary=:all: --require-hashes -r scripts/monitoring/auth-requirements.txt
```

Provide operator-managed certificates and private keys for each native server,
trusted CA files, and a reviewed HTTPS receiver with request authentication.
Use an explicit verified server name matching each server certificate. The
renderer accepts numeric nonzero loopback listener/scrape sockets, an HTTPS
receiver URL with a path, and these exact manifest keys:

```json
{
  "prometheus": {
    "listen": "127.0.0.1:9090",
    "server_name": "prometheus.monitor.example",
    "ca_file": "/etc/26chan/monitoring/prometheus-ca.pem",
    "cert_file": "/etc/26chan/monitoring/prometheus-cert.pem",
    "key_file": "/etc/26chan/monitoring/prometheus-key.pem",
    "operator_password_file": "/etc/26chan/monitoring/prometheus-operator.password"
  },
  "alertmanager": {
    "listen": "127.0.0.1:9093",
    "server_name": "alertmanager.monitor.example",
    "ca_file": "/etc/26chan/monitoring/alertmanager-ca.pem",
    "cert_file": "/etc/26chan/monitoring/alertmanager-cert.pem",
    "key_file": "/etc/26chan/monitoring/alertmanager-key.pem",
    "ingest_password_file": "/etc/26chan/monitoring/alertmanager-ingest.password",
    "operator_password_file": "/etc/26chan/monitoring/alertmanager-operator.password"
  },
  "receiver": {
    "url": "https://receiver.monitor.example/alerts",
    "ca_file": "/etc/26chan/monitoring/receiver-ca.pem",
    "token_file": "/etc/26chan/monitoring/receiver.token"
  },
  "scrapes": [
    {"job": "board-public", "target": "127.0.0.1:9191", "token_file": "/etc/26chan/metrics/public.token"},
    {"job": "board-staff", "target": "127.0.0.1:9192", "token_file": "/etc/26chan/metrics/staff.token"},
    {"job": "board-media", "target": "127.0.0.1:9193", "token_file": "/etc/26chan/metrics/media.token"},
    {"job": "board-monitor", "target": "127.0.0.1:9194", "token_file": "/etc/26chan/metrics/monitor.token"},
    {"job": "board-resource", "target": "127.0.0.1:9195", "token_file": "/etc/26chan/metrics/resource.token"}
  ],
  "rules_file": "/etc/26chan/monitoring/alerts.yml"
}
```

These are example names, not deployed services. Include only enabled scrape jobs
(one to five unique jobs). Generate independent cryptographically random 32-byte
values and hex-encode them into private files: exactly 64 lowercase hex characters,
optionally followed by a newline. Every password/token must differ, including
scrape tokens. Never put raw credentials in command arguments, logs or commits.

All input paths must be absolute, canonical, regular non-symlink files. Private
keys and credential files must deny group/other access on POSIX. Configure
equivalent ACLs on Windows; the renderer does not establish Windows ACL isolation.
It bounds JSON to 64 KiB, keys/certificates to 128 KiB and rules to 256 KiB, rejects
duplicate/unknown fields and validates before writing. It does not contact the
receiver, validate a certificate chain or establish the recipient's authority.
The native checks and owned deployment qualification remain necessary.

Create a private manifest and choose a new output directory beneath an existing
operator-owned parent. Existing output directories are refused:

```bash
.local/monitor-auth-venv/bin/python scripts/monitoring/auth_profile.py --manifest /etc/26chan/monitoring/profile.json --output /etc/26chan/monitoring/staged-profile
.local/monitoring/bin/promtool check config /etc/26chan/monitoring/staged-profile/prometheus.yml
.local/monitoring/bin/promtool check web-config /etc/26chan/monitoring/staged-profile/prometheus-web.yml /etc/26chan/monitoring/staged-profile/alertmanager-web.yml
.local/monitoring/bin/amtool check-config /etc/26chan/monitoring/staged-profile/alertmanager.yml
```

The four files are JSON accepted as YAML by the pinned native tools. Output
directory mode is 0700 and files 0600. Web policies contain bcrypt cost 12 hashes,
never raw Basic passwords. Outbound credentials remain in their referenced
private files. Both HTTPS clients verify trusted CAs and names, require TLS 1.3
and disable redirects. Server web policies require TLS 1.3 and nonempty Basic users.

Distribute only each service's config, server key, required CA files and outbound
secrets into its own private directory. Prometheus needs its exporter tokens and
Alertmanager ingest password; Alertmanager needs the receiver token. Neither
service needs operator plaintext passwords. A staging bundle accessible to a
single operator is not a runtime directory shared by both service identities.

Run each native binary under its reviewed service identity with the corresponding
`--config.file`, `--web.config.file` and manifest `--web.listen-address`. Set bounded
storage retention and host resources. Alertmanager additionally requires
`--cluster.listen-address=` to disable gossip. Keep monitoring endpoints off public
ingress. Operator clients must validate the intended CA and server name and supply
their own Basic credential. Do not enable lifecycle or admin APIs by default.

## Rotation and policy failure

Stage a complete replacement using the renderer in a new private directory. Run
all native validation commands against the staged files as the relevant service
identity so unreadable dependencies are detected. Review paths and permissions,
then atomically replace the complete affected web-policy file in the same
filesystem. Never truncate a live policy, publish a partial file or deploy an empty
`basic_auth_users` map: the native toolkit deliberately treats an empty map as
authentication disabled. A trusted operator can change policy; rendering does not
protect against that operator.

Native Basic policy is read on each request. Revoking a password denies the old
credential on an already established TLS connection. An unavailable policy
returns 500 on an existing connection; a new TLS handshake may fail instead.
The owned test proves recovery after restoring the complete valid policy.
Coordinate the outbound password-file replacement with the server policy and
verify an allowed request plus rejection of the old credential. Credential files
must also be replaced atomically with private permissions. The native format has
no password-expiration field: scheduled rotation and incident revocation belong
to the operator, not an undocumented expiry timer.

Native certificate configuration reload happens for new TLS connections. A
certificate/CA replacement alone does not immediately revoke existing
connections. For incident response, revoke request credentials and stop/restart
the affected services as required to terminate established sessions; requalify
their new peer identity before resuming delivery. Review the production receiver's
actual expiry, revocation and outage behavior separately. The test receiver's
per-request hash policy is test support and is not a production service.

## Qualification and limits

The focused tests create private synthetic PKI with OpenSSL and exercise missing,
wrong, expired, revoked and unavailable receiver policy plus wrong-CA,
wrong-hostname and expired-server TLS rejection. The real transport uses the Rust
HTTP fixture, Prometheus 3.14.0 and Alertmanager 0.34.0 with the generated profile.
It requires authenticated API access, existing-connection policy revocation,
actual denied sends on both links, restored firing delivery and matching recovery.

```bash
.local/monitor-auth-venv/bin/python -m unittest discover -s tests/monitoring/authenticated -p 'test_*.py'
.local/monitor-auth-venv/bin/python tests/monitoring/authenticated/qualify.py
.local/monitor-auth-venv/bin/python tests/monitoring/authenticated/interruption.py
```

The final command requires Linux and delivers OS SIGTERM after a real healthy
scrape. It checks owned child processes, the receiver listener, private PKI and
temporary storage disappear. The normal run stops/reaps owned children as well.
For transport timing only, scrape/evaluation/group intervals become 1s, the HTTP
rate window 30s and the HTTP pending interval 4s. Production expressions, thresholds
and grouping remain unchanged; native tools also validate the unmodified output.
See the [verification record](verification-authenticated-monitoring.md) for actual
results rather than treating these commands as proof they ran.

An owned local webhook is not a production notification destination. Receiver
ownership, escalation, durable monitoring storage, independent service identities,
deployed ingress/egress policy, resource enforcement, host/database storage and
update-check monitoring still require qualification before launch.

Sources: [native HTTPS/authentication](https://prometheus.io/docs/alerting/latest/https/),
[Prometheus configuration](https://prometheus.io/docs/prometheus/latest/configuration/configuration/),
[Alertmanager configuration](https://prometheus.io/docs/alerting/latest/configuration/),
[pinned per-request policy handler](https://github.com/prometheus/exporter-toolkit/blob/v0.17.1/web/handler.go),
[pinned TLS reload](https://github.com/prometheus/exporter-toolkit/blob/v0.17.1/web/tls_config.go).
