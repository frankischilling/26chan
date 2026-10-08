# Paired media runner qualification

This suite qualifies the inactive paired-v2 candidate path on the existing owned,
disposable Linux runner. It requires root, KVM, systemd, the pinned artifacts
prepared by `scripts/media/provision-test.py`, and the migrated disposable test
database. It must not run on a production host or a shared queue.

The suite has not been run locally. Passing fixture or transport unit tests does
not qualify Firecracker execution. A candidate remains non-authoritative even
when every check passes: no replay admission, browser rendering, PNG/replay
parity, asset approval, output installation, or publication is enabled.

## Entry points

Use `scripts/test-paired-media-dispatch.sh` for the full privileged run. It checks
root, the exact `MEDIA_PAIRED_VM_QUALIFY=1` opt-in, explicit
`APP_ENV=development`, absolute
artifact paths, private root-owned configuration files, and the disposable
PostgreSQL cluster identity. It runs the VM suite with a cleared environment,
then sources the existing database, media, reader and intake credential files
for dispatch qualification. It removes Python startup environment controls and
ignores them for the dispatch interpreter. Direct Python entries reject `-O`
and `-OO`, production or missing development mode, and missing opt-in before
any VM, service or SQL work. The bounded Unix-client helper separately requires
its authorized nonroot gateway identity and explicit client-mode flag.
The script does not create or rotate credentials.

- `python3 -m unittest discover -s tests/media -p test_paired_vm_controls.py -v`
  checks the deterministic fixtures and the independent result assertion without
  root or a VM.
- `python3 tests/media/test_paired_vm.py -v` requires
  `MEDIA_PAIRED_VM_QUALIFY=1`, `MEDIA_VM_TEST_CONFIG` pointing to the provisioned
  `decode.json`, and `MEDIA_VM_PROBE_CONFIG` pointing to the separately provisioned
  `probe.json`. Use a cleared environment, as for the existing image-v1 suite.
- `python3 tests/media/test_paired_dispatch_vm.py` requires those same variables,
  `MEDIA_DISPATCH_BIN_DIR`, and `MEDIA_PAIRED_COORDINATOR_TEST` pointing to the
  compiled `board-media-admin` integration-test binary `paired_vm`, built with
  `--features database-tests --test paired_vm --no-run`. It also requires the
  existing disposable `MIGRATION_DATABASE_URL`, `MEDIA_DATABASE_URL`,
  `MEDIA_READ_DATABASE_URL`, and `INTAKE_DATABASE_URL` settings. Use the existing
  disposable database verification before launching it.

The orchestrator supplies the native test with private `MEDIA_PAIRED_CLIENT_CONFIG`
and `MEDIA_QUARANTINE_DIR` paths. It runs the ignored
`real_guest_candidates_remain_nonpublishable` test as the separate coordinator
identity. Root broker and nonroot Rust gateway receive cleared environments;
only the coordinator test receives database credentials. No production command
or route is added for paired processing.

## Checks

The decode suite boots the actual guest init and decoder, passes the selected
paired-v2 disk through the real runner, and compares the stopped result with
independent frozen RGBA/replay wire bytes. It covers PNG-only requests, both
recorder fixtures, distinct PNG and replay dimensions, fresh bindings, flags,
lengths, zero padding, and fixed result size.

The exact input-cap fixture contains an 8 MiB PNG with an ancillary chunk and an
8 MiB TGKR stream. Legal empty raw-DEFLATE blocks expand the latter's compressed
representation without changing its recorded body. Both together fill the
16,777,272-byte frame. The VM must decode it successfully under the current
limits; rejection is not counted as qualification. A separate 1024-square image
exercises maximum RGBA output, and a 1025-pixel dimension must reject.

Malformed PNG/TGKR, wrong inner framing, presence disagreement, component-size
violations and bad trailers must leave an all-zero result disk. Selected tests
alter the real input disk after runner framing to check nonzero padding, extra
sectors, truncated content, wrong outer version and out-of-range declarations
inside the guest. Only input bytes are changed; Firecracker, init, decoder,
resource controls and result collection are unchanged.

The separate containment-probe initramfs accepts a bounded explicit v2 command
envelope for inspect, memory, process, disk, CPU and wall-clock controls. Its
reports are not paired candidates. The existing v1 commands and tests remain.
The selected fixed result disk and file-size ceiling are 4,456,960 bytes for v2;
CPU, address space, process, VM memory, systemd, tmpfs and timeout limits do not
increase.

The broker suite makes actual requests as its authorized gateway UID. It checks
valid pairs and the exact combined cap, malformed and oversized framing, missing
EOF under the existing three-second intake deadline, and recovery with another
real job. The native coordinator test then crosses SQL intake, typed lease,
immutable snapshot, Rust mTLS client, Rust gateway, root broker and the actual
guest. It checks the returned candidate independently and verifies that successful
transport ends in `failed/candidate_checked` with no output metadata or assets.
Invalid component cases must fail without publication. Existing root-gated Rust
TLS tests additionally cover request EOF before broker access, binding and
version preservation, downgrade rejection and authorization boundaries.

Run all existing image-v1 VM, boundary, recovery, gateway and publication tests
alongside this suite. These additions do not replace their coverage.
