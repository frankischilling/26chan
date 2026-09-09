# Media quarantine and job control

The September 8 implementation prompt authorizes this next slice. The public text foundation is merged at `8f3cdb4`. No separate design brief has been located. The existing compatibility target and rendering stay in place.

This slice implements storage and job control without enabling public uploads or executing a decoder. A maintained per-job microVM, reviewed host and guest artifacts, and deployed containment evidence are required before either is enabled. WSL exposes `/dev/kvm`, but this checkout has no installed Firecracker runtime or approved guest image. KVM presence alone does not establish the boundary.

## Responsibilities

`board-media` provides server-generated opaque object IDs, streaming bounded quarantine intake, and a fixed untrusted-output protocol. Intake writes a temporary file, enforces the byte ceiling during each read, and finalizes only complete nonempty objects. Failed and cancelled intake must remove its partial file. Filenames remain bounded display metadata and never select a path.

The worker-output protocol is `IBRGBA01` followed by unsigned big-endian 32-bit width and height and exactly `width * height * 4` RGBA bytes. Both dimensions are between 1 and 1024. No filenames, executable actions, free-form metadata, compressed payloads or filesystem structures cross this boundary. The validator checks the header before allocating and rejects truncation and trailing bytes. An encoder converts validated pixels to PNG with no supplied ancillary metadata. Only a validated value can reach the promoter. PNG parsing is used only in tests to verify generated artifacts.

Promotion publishes by generated ID using atomic no-clobber persistence. Identical repeated output is idempotent; conflicting output is an error. Public storage is distinct from quarantine. Roots are trusted operator-owned directories, inaccessible to the worker. These library filesystem checks do not substitute for deployment permissions against a compromised host process.

`media.jobs` persists intake reservations, queued jobs, processing leases, published results and terminal failures. Database constraints bound metadata and state. Transactional queue admission enforces a fixed capacity including unfinished intake. Claims increment an attempt counter and use a random lease token. Completion, failure and retry require the current unexpired token; expired/stale tokens cannot mutate a replacement attempt. Retries are bounded, and abandoned intake/leases expire. Public credentials have no media schema access. A separate media login can operate only on the queue; it cannot access content, deletion secrets, staff identities or deployment settings.

An operator-only development command supports bounded intake, status and expired-job cleanup using the media login. It does not run subprocesses, accept worker commands, publish arbitrary files, or provide an unsandboxed processing option. Integration tests connect real queue state, private storage, synthetic pixel output and promotion. They establish plumbing properties, not an isolated processing boundary.

## Failure and retention policy

Intake reserves queue space before consuming bytes. A failed stream marks its reservation failed and removes the partial file. Process interruption leaves a time-limited reservation; cleanup reconciles terminal jobs with private files. Terminal metadata and retained input need finite retention and an operator cleanup command. Publication never falls back to the original upload. Originals have no download route. A retry cannot overwrite an existing different artifact.

## Acceptance

- Real streaming limits, partial-file cleanup, invalid-ID rejection and fixed-schema output validation have bounded unit/property tests with harmless data.
- Real PostgreSQL tests enforce admission under concurrent reservations, exclusive claim, stale/expired fencing, bounded retries and access denials with allowed positive controls.
- The local intake/status/cleanup command works with the separately provisioned media credential and rejects inherited operator/staff/public credentials.
- CI provisions and exercises the real media role; the restore exercise covers new persisted state and restored denials.
- Documentation distinguishes passing library/database checks from unrun microVM, networking, external CPU/memory/disk/process/runtime and guest cleanup checks.

Public attachment integration, decoders, isolated execution, staff WebAuthn, reference visual parity and production launch remain separate acceptance criteria from the mission. They must not be marked complete by this slice.
