# Storage and resource observation

Prompt section 11 requires storage/resource pressure alerts. Add a Linux-only
`board-resource-monitor` process with read-only filesystem/cgroup observations,
the existing private metrics endpoint and real pressure/delivery qualification.
This slice does not grant database or service-control authority. Update-check
monitoring remains a separate implementation because it consumes advisory/job state.

## Choice and authority

A general host exporter adds a separate broad collection/configuration surface;
app-local probes give serving processes unrelated host paths. Use a small separate
Rust observer, reusing board-observe, Tokio, Serde and already locked Rustix 1.1.4.
No registry version changes. First-party Rust remains safe. Parsing and native
syscalls are isolated from the public/staff/media applications.

The observer receives only its metrics credential and an operator-owned JSON
path configuration. It never opens application data files, enumerates processes,
queries SQL, controls a cgroup, mounts a filesystem or makes outbound requests.
Its unique OS identity must lack application/staff data and control authority.
Candidate systemd configuration keeps cgroups read-only, clears capabilities and
restricts resources. Storage mount flags must match the writer's view: marking
an observer bind mount read-only would always report read-only storage. Explicit
exceptions for only the configured storage directories preserve those flags;
root-controlled directory/file permissions deny the distinct observer identity
payload reads and all writes. Actual denied operations and positive controls are
required; this is not approval of a production host.

## Configuration and measurements

Require explicit APP_ENV=development or production, RESOURCE_CONFIG_FILE and the
existing METRICS_BIND_ADDR/METRICS_TOKEN pair. Runtime rejects known unrelated
database credentials and PG* overrides, including non-Unicode values. Linux-only
runtime; Windows supports parser/cache/exposition tests and rejects startup.

The canonical absolute regular non-symlink configuration file is at most 16 KiB.
Strict JSON has exactly two arrays, each nonempty:

```json
{"storages":[{"target":"database","path":"/var/lib/postgresql/16/main"}],
 "services":[{"target":"public","path":"/sys/fs/cgroup/system.slice/board-public.service"}]}
```

Paths must be canonical absolute directories with no symlink components. Targets
are unique closed labels; repeated paths are allowed because multiple storage
purposes may share a filesystem. Do not emit paths, device names or process IDs.

Storage order (maximum 4): database, quarantine, public_media, monitoring.
Service order (maximum 10): public, staff, media_gateway, media_broker, media_reader,
queue_observer, resource_observer, database, prometheus, alertmanager.

Each sample reopens checked directories. Storage uses an O_PATH directory handle
and fstatvfs, without reading directory entries or payload files. Record total and
unprivileged-available bytes, total/available inodes and read-only state. Require
positive totals and checked multiplication; reject impossible available totals.
These describe the containing filesystem, not directory size, project quotas,
database growth, IOPS or backup durability. Reserved blocks are not available.
Read-only describes the observer's mount view; qualify its agreement with the
writer's view and real transitions before treating this as a volume-failure alert.

Cgroups must have cgroup v2 filesystem magic. Open only fixed files relative to
the checked directory, no symlinks or blocking special files, at most 4 KiB each:
memory.current, memory.max, memory.events, pids.current, pids.max, cpu.stat, cpu.max.
Require finite positive local memory/task/CPU limits. Parse unsigned decimal
values and required unique keys; reject missing, duplicate, overflowing or invalid
required data. Allow new unknown stat/event keys for kernel compatibility.
Read memory OOM-kill count and CPU usage/period/throttled-period counters. Values
can reset when a service cgroup is recreated. Local ceilings are not the effective
minimum of ancestor limits; ancestor contention can constrain services earlier.
The observations are not an atomic kernel snapshot.

## Bounded sampling and metrics

Sample immediately then every 5 seconds, skipping missed ticks. Use at most one
blocking collection operation; timeout after 2 seconds must not release its
admission while it still runs. No overlapping replacement jobs or unbounded
blocking threads. Stop cancels waiting; runtime shutdown must not wait forever
for an uninterruptible kernel operation. The candidate unit also bounds stop time.

Cache a complete fixed-size snapshot. Any source failure makes the whole snapshot
unavailable immediately; age above 30 monotonic seconds also invalidates it.
On failure retain only last-success time, omit data families, return authenticated
readyz503, and continue serving healthz200. Scraping never reads the filesystem.
Reuse existing endpoint authentication, response retention, connection/deadline
bounds. No fallback to stale healthy-looking numbers.

Public board-observe types (Clone, Copy, Debug, Default):

```text
StorageSample { capacity_bytes:u64, available_bytes:u64, inodes:u64,
                available_inodes:u64, read_only:bool }
ServiceSample { memory_bytes:u64, memory_limit_bytes:u64, tasks:u64,
                tasks_limit:u64, cpu_usage_usec:u64, cpu_quota_usec:u64,
                cpu_period_usec:u64, cpu_periods:u64,
                cpu_throttled_periods:u64, memory_oom_kills:u64 }
ResourceSample { available:bool, last_success_timestamp_seconds:u64,
                 storages:[Option<StorageSample>;4],
                 services:[Option<ServiceSample>;10] }
STORAGE_TARGETS:[&str;4]; SERVICE_TARGETS:[&str;10] in the orders above.
Metrics::register_resources(callback: Fn()->ResourceSample +Send+Sync+'static)
    -> Result<(),RegistrationError>
```

Always export board_resource_sample_success and
board_resource_sample_last_success_timestamp_seconds. When available, configured
targets export these fixed families:

```text
board_storage_capacity_bytes{storage}
board_storage_available_bytes{storage}
board_storage_inodes{storage}
board_storage_available_inodes{storage}
board_storage_read_only{storage}
board_service_memory_bytes{service}
board_service_memory_limit_bytes{service}
board_service_tasks{service}
board_service_tasks_limit{service}
board_service_cpu_usage_seconds_total{service} (microseconds / 1e6)
board_service_cpu_quota_cores{service} (quota / period)
board_service_cpu_periods_total{service}
board_service_cpu_throttled_periods_total{service}
board_service_memory_oom_kills_total{service}
```

Production rules: observer unavailable/stale for30s; storage available bytes or
inodes below10% for2m; storage read-only for30s; memory/tasks above90% for2m;
CPU throttled-period ratio above20% over5m for2m; OOM-kill increase over5m for30s.
All pressure rules require a healthy fresh sample and up=1. Counter rules handle
resets. Rule tests cover healthy, pending, firing, resolution, unavailable/stale
suppression, zero denominators and resets. The authenticated profile accepts the
fifth closed job board-resource with its own token; grouping adds storage/service.

## Actual qualification

Use an owned Linux helper with a generated private directory, a small tmpfs
(16 MiB, bounded inode count), limited transient fixture service, distinct
unprivileged observer service and exact owned cleanup. Never pressure the host's
root filesystem or global memory, and never stop unrelated services. The observer
gets no root credential, database login, service-control socket or application
data reads. Keep public uploads and production media disabled.

Require actual healthy metrics and denied missing/wrong token requests. Fill and
clear only the owned mount to cross the unchanged byte and inode thresholds.
Hold/release bounded memory and tasks inside the fixture cgroup; run a bounded
CPU workload under its finite quota. Check actual metric changes and firing plus
resolved notifications through the existing authenticated HTTPS profile. Use
synthetic local PKI/credentials and pinned real Prometheus/Alertmanager. Accelerate
only rule/transport time constants, not thresholds. OOM/read-only/unavailable
cases need real controlled source evidence plus rule tests; do not label fabricated
numeric files as native cgroup evidence.

Verify observer cannot read harmless protected payloads or write cgroup limits,
while permitted statistics and fixture controls work. Test unavailable source
and recovery, normal stop, OS SIGTERM cleanup, listener closure, empty owned
service cgroups and removed temporary credentials/mounts. Keep a live owned
supervisor through abnormal cleanup; no reused PID/process-name cleanup.

CI must exercise Linux native collection and actual qualification in addition to
portable unit/property tests, existing monitoring, and full application CI.
Record implementation reviews, commands, results, limits and checked-head merge.

Sources: [Rustix fstatvfs](https://docs.rs/rustix/1.1.4/rustix/fs/fn.fstatvfs.html),
[Linux cgroup v2 interface](https://docs.kernel.org/admin-guide/cgroup-v2.html).
