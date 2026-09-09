# Media intake and job control

Public uploads are disabled. `board-media-admin` is a development operator command for testing private intake and cleanup. A separate [Firecracker qualification profile](firecracker.md) runs a Rust decoder inside a disposable guest and validates its bounded output. Its actual VM tests provide local evidence; library and queue tests alone do not establish that boundary.

## Local setup

After creating the disposable database, provision its separate media login before running migrations:

```bash
sudo bash scripts/dev-media-db.sh
sudo chown "$(id -u):$(id -g)" .local/media.env .local/media.ps1
source .local/database.env
cargo run -p board-store --bin board-migrate --locked
```

The provisioning script verifies the cluster directory on port 55432, refuses to overwrite credential files, and writes generated secrets only under ignored `.local/`. It is a development bootstrap, not a production secret manager. Use a new shell for media commands, or remove public, test, staff, authentication and migration credentials first:

```bash
unset DATABASE_URL TEST_PUBLIC_DATABASE_URL MIGRATION_DATABASE_URL STAFF_DATABASE_URL AUTH_DATABASE_URL
source .local/media.env
export MEDIA_QUARANTINE_DIR="$PWD/.local/quarantine"
printf 'harmless undecoded test bytes' > .local/input.txt
cargo run -p board-media-admin --locked -- intake .local/input.txt example.png
# Use the generated ID printed by intake:
cargo run -p board-media-admin --locked -- status OBJECT_ID
cargo run -p board-media-admin --locked -- cleanup
```

On Windows use `. .\.local\media.ps1` and set `$env:MEDIA_QUARANTINE_DIR` to an absolute private path. The input file path is chosen by the operator; the display filename never selects a storage path. No bytes are decoded by this command, and renaming the fixture to `.png` does not make it an image. `APP_ENV=production` and `MEDIA_ENABLED=true` both fail startup. Public startup rejects the media credential too.

Use one consistently configured quarantine root for each queue. Roots and their parents must be controlled by the operator and inaccessible to workers and public serving. Filesystem path checks do not enforce permissions against a hostile local process. The library requires same-filesystem atomic hard links and fails if the filesystem cannot provide them. It does not guarantee directory-entry durability across power loss. Back up a stopped, reconciled queue and its private root together if retaining development jobs matters.

## Limits and states

| Control | Implemented value |
|---|---|
| Intake byte limit | 8 MiB, checked during 8 KiB reads with at most one overflow lookahead byte |
| Operator intake deadline | 15 seconds around asynchronous reads; synchronous filesystem operations can delay cooperative deadlines |
| Display filename | 1 to 255 UTF-8 bytes, no control characters; not included in command status logs |
| Queue capacity | 64 unfinished jobs by default, serialized admission; operator can set 1 to 1,024 |
| Receiving deadline | 5 minutes |
| Queued deadline | 1 hour; an unavailable worker cannot retain a pending job forever |
| Processing lease | 30 seconds and a fresh opaque token on each claim |
| Attempts | At most 3; deterministic invalid output can fail immediately |
| Cleanup | At most 64 expired jobs and 64 terminal records per invocation; terminal input retained 1 day before removal |
| Worker output | 16-byte header plus at most 4 MiB raw RGBA pixels and one EOF lookahead |
| Encoded output | At most 5 MiB of generated PNG bytes |

The transitions are `receiving -> queued -> processing -> published` or `failed`. A processing failure or abandoned lease can return to `queued` while attempts remain. Every completion/failure requires the current unexpired lease token. Exact duplicate completion is idempotent; different output receipts cannot replace a published receipt. Cleanup deletes private input before terminal metadata and can be repeated after a failure. Queue capacity does not cap accumulated terminal files; a production filesystem quota and scheduled cleanup remain required.

Cancellation and ordinary stream errors remove partial files. Abrupt process termination can leave a `{id}.part` or completed private input attached to its reservation. Expiration and terminal cleanup reconcile those exact generated names. Cleanup must use the same root as intake. Metadata and filesystem writes are not one atomic transaction: if queue finalization returns an uncertain database outcome, intake preserves the private object for reconciliation. There is no running worker or attachment-serving adapter that resolves publication crashes yet.

## Untrusted output

The protocol is eight ASCII bytes `IBRGBA01`, then width and height as unsigned big-endian 32-bit integers, then exactly `width * height * 4` RGBA bytes, followed by EOF. Each dimension must be 1 through 1,024. A caller must apply an external job deadline to a stream that never supplies EOF. The validator allocates only after checking dimensions and rejects truncated or trailing data.

The promoter accepts only the library's validated type. It encodes PNG without supplied ancillary metadata, caps the actual encoder output, and creates `{generated_id}.png` without overwriting. An identical replay succeeds; a different existing object fails. Neither success flags nor filenames, archive structures, commands or paths are accepted from a worker. No original-file download route exists and failed processing never publishes the upload.

The PNG encoder runs in the future promotion trust boundary, not in the public application. The `png` dependency also contains decoder APIs, but runtime code does not invoke them; test-only decoding verifies generated pixels. That dependency and its compression code remain part of the promotion trust base. Temporary `.publish-*` files use the destination filesystem for atomic linking. Public storage must serve only the approved generated-name pattern and deny dotfiles; abrupt termination can leave staging files for operator reconciliation. A production adapter must fence publication against lease replacement and handle the database/filesystem crash window before attaching this output to public posts.

## Privilege and containment evidence

`board_media` can read queue policy, lock its singleton admission row, and read/write/delete media job records. It cannot change capacity, select content or deletion hashes, access staff identities or deployment settings, create a schema, or assume the migration role. Tests execute these denials against the actual login and confirm the public login can read its own content but cannot access the media queue. A compromised coordinator with this credential can corrupt or exhaust its queue; it has no content or staff database authority.

The [local Firecracker profile](firecracker.md) executes a real guest with pinned runtime/kernel artifacts and per-job raw input/output devices. Tests cover its stated worker access, resource and ordinary-cleanup behavior. They do not qualify a production processing tier or prove every required metadata/DNS/other-job/storage path. Firecracker requires KVM plus explicit host, jailer and resource configuration; see its [production host guidance](https://github.com/firecracker-microvm/firecracker/blob/v1.16.1/docs/prod-host-setup.md). Authenticated queue dispatch, publication fencing, abrupt supervisor-crash reconciliation and independent deployed review remain required.

On an owned deployment, record artifact hashes and service identities before each containment run. Start a harmless internal test service and prove it is reachable from an allowed control host. Then test access from the actual worker context to that service, database listeners, synthetic metadata and DNS services, a second job's harmless input, unauthorized storage and host-management interfaces. Record denied outcomes and confirm the positive controls remain healthy. Inspect only the expected credential-variable names and descriptor targets, without printing secrets. Use small fixed-duration workloads to exercise each external resource ceiling, verify the entire job process/cgroup is gone afterward, and confirm its workspace cannot be reused. The [execution evidence](verification-firecracker.md) records the local subset that ran; the complete deployed acceptance set remains unfinished and cannot be replaced by string checks on deployment files.

## Tests

```bash
source .local/database.env
source .local/media.env
cargo test -p board-media -p board-store -p board-media-admin --all-features --locked
sudo bash scripts/restore-exercise.sh
```

Use an idle disposable media queue. The concurrency test refuses to start with existing unfinished jobs. Pixel frames and uploads are synthetic and harmless. The integration test joins private intake, actual queue claims, failed-output rejection and idempotent PNG receipts; it executes no decoder or VM. The restore exercise checks the new table counts and rechecks restored media-role denials. Production restore durability and backup deletion isolation remain unverified.
