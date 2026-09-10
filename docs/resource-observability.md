# Storage and service resource observation

`board-resource-monitor` is a separate Linux process that reads configured
filesystem statistics and cgroup v2 counters. It exposes fixed metrics through
the existing authenticated loopback endpoint. It has no database client or
service-control credentials. The [verification record](verification-resource-observability.md)
distinguishes completed checks from pending native qualification.

## Configuration and authority

Set `APP_ENV=production` (or explicit `development`), `RESOURCE_CONFIG_FILE`,
`METRICS_BIND_ADDR=127.0.0.1:9195`, and a separate `METRICS_TOKEN`. Follow the
[private endpoint credential requirements](http-observability.md). Missing
endpoint settings, command-line arguments, known database URL variables and any
`PG*` environment override are rejected; empty credentials are still rejected.
Windows startup fails rather than supplying substitute measurements.

The configuration is a canonical absolute, non-symlink regular file of at most
16 KiB. Use operator-controlled ownership and directory permissions. Both arrays
must be nonempty; unknown fields, duplicate fields/targets and invalid paths fail.
For example, after verifying the actual host directories:

```json
{
  "storages": [{"target": "database", "path": "/var/lib/postgresql/16/main"}],
  "services": [{"target": "public", "path": "/sys/fs/cgroup/system.slice/board-public.service"}]
}
```

Storage labels are `database`, `quarantine`, `public_media`, `monitoring`.
Service labels are `public`, `staff`, `media_gateway`, `media_broker`,
`media_reader`, `queue_observer`, `resource_observer`, `database`, `prometheus`,
`alertmanager`. Repeated paths across distinct labels are allowed; no paths,
process IDs or device names appear in the metrics.

Each configured directory must be canonical, absolute and free of symlink
components. Filesystem observation uses directory handles without enumerating
entries or opening payloads. The observer needs traversal to reach each source,
but no permission to read data or change files. Cgroup reads use only seven fixed,
bounded regular files under checked cgroup v2 directories. Set finite positive
local memory, task and CPU ceilings for every observed service; an unlimited or
missing ceiling makes the complete sample unavailable.

The [candidate service](../deploy/resource-monitor.service) uses a unique OS
identity, empty capabilities and bounded memory/tasks/CPU. Its exact storage
`ReadWritePaths` exceptions must match the host configuration. Those exceptions
preserve source mount flags; root-controlled directory/file permissions must
deny the observer all storage writes and payload reads. Adding the observer to
an application data group can grant unintended read access. Qualify actual
denials, traversal/statistics access and writer controls on the owned host.

`ProtectSystem=strict` without matching exceptions makes writable storage look
read-only inside the observer namespace. Conversely, a writer may have its own
read-only bind mount. Verify writer and observer mount views and real read-only
transitions before interpreting this alert as a shared volume failure. Cgroups
remain read-only to the observer. The unit is a candidate, not a deployed network
policy or a substitute for host permission checks.

## Sampling and alert behavior

The observer samples immediately and every five seconds. Each complete operation
has a two-second deadline. A timed-out blocking call retains the sole admission
slot until it exits; later ticks cannot accumulate replacement threads. Runtime
shutdown waits at most one second for blocking teardown, with the service unit
providing a separate stop deadline.

A failed source invalidates the entire sample. A cache older than 30 monotonic
seconds is unavailable. In either case data families disappear, last-success
time remains, authenticated `/readyz` returns 503 and `/healthz` remains 200.
Scraping reads only bounded cached data and cannot trigger filesystem work.

Storage values describe the containing filesystem: total and unprivileged
available bytes, total/available inodes and read-only state. They do not measure
directory size, project quotas, database growth, I/O latency or backup durability.
Reserved blocks are excluded from available bytes. Cgroup limits describe local
ceilings, not the effective minimum imposed by ancestors. Ancestor contention
can constrain a service earlier. Reads are not an atomic kernel snapshot;
cumulative CPU and OOM counters may reset when a cgroup is recreated.

The existing [rules](../deploy/monitoring/alerts.yml) add:

| Condition | Threshold and duration |
|---|---|
| Observer unavailable or stale | 30 seconds |
| Available storage bytes or inodes | Below 10% for 2 minutes |
| Storage read-only | 30 seconds |
| Service memory or tasks | Above 90% of local ceiling for 2 minutes |
| CPU throttled periods | Above 20% over 5 minutes, held for 2 minutes |
| OOM kills | Increase over 5 minutes, held for 30 seconds |

Pressure alerts require successful fresh samples and a successful scrape.
The authenticated renderer accepts a fifth closed job, `board-resource`, with
an independent token and grouping by storage/service. Keep it in the monitored
target inventory: an omitted target cannot generate an `up`-based outage alert.
Use [authenticated monitoring](authenticated-monitoring.md) for both native
server links and notification delivery. Production thresholds require workload
review; CI pressure is confined to a disposable mount and limited fixture cgroup.

## Owned qualification

On a disposable Linux systemd/cgroup v2 runner, build the observer, download the
pinned monitoring tools, and install the hash-pinned profile dependency into the
ignored virtual environment. Run the normal and interruption cases as CI does:

```bash
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
  RESOURCE_QUALIFICATION_PYTHON="$PWD/.local/monitor-auth-venv/bin/python" \
  bash scripts/test-resource-monitoring.sh "$PWD/target/debug/board-resource-monitor" "$PWD/.local/monitoring/bin"
sudo env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin RESOURCE_QUALIFICATION_INTERRUPT=1 \
  RESOURCE_QUALIFICATION_PYTHON="$PWD/.local/monitor-auth-venv/bin/python" \
  bash scripts/test-resource-monitoring.sh "$PWD/target/debug/board-resource-monitor" "$PWD/.local/monitoring/bin"
```

The helper creates only its own small mount, limited service, private credentials
and distinct observer identity. It must prove actual metrics, denied requests,
firing/resolved HTTPS notifications, source recovery and exact cleanup. Time
constants are shortened for qualification; pressure thresholds remain unchanged.
This does not qualify production infrastructure or enable public uploads.
