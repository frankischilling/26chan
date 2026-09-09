# Firecracker execution implementation plan

Use the subagent-driven-development skill for the independent block protocol task and its review. The lead owns the Linux runner, guest integration, infrastructure tests and documentation in the requested current checkout.

Goal: run one isolated media job, validate its bounded result and establish reproducible local containment evidence before queue/public enablement.

Spec: [firecracker-design.md](firecracker-design.md).

## Global constraints

- Keep first-party Rust safe with `#![forbid(unsafe_code)]`.
- Use Firecracker 1.16.1 and its matching jailer with a recorded checksum.
- Input is 1 through 8 MiB. Output is exactly 4,194,816 bytes, existing IBRGBA01 pixels then zero padding; dimensions are 1 through 1,024.
- Do not mount or parse guest filesystems on the host. Supply no network/vsock/API interface, credentials or shared job directories to the guest.
- Use an owned disposable Linux test environment. Production and public media enablement remain rejected until all required acceptance evidence exists.
- Use the configured Git identity and current repository, synthetic fixtures and accurate verification records. Do not merge or deploy production.

## Task 1: Bounded block protocol

Files: `crates/media/src/block.rs`, `crates/media/src/lib.rs`, `crates/media/tests/block.rs`.

Interfaces: `write_input_disk<R: std::io::Read, W: std::io::Write>(reader: R, bytes: u64, writer: W) -> Result<(), MediaError>`; `ValidatedOutput::read_disk<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Self, MediaError>`; exported `OUTPUT_DISK_BYTES: u64`.

- [ ] Write failing tests using a hand-built 1x1 red pixel disk, including bad padding, trailing bytes, truncation, excessive dimensions and every input length mismatch. Before implementation the valid disk read must fail or be unavailable.
- [ ] Implement exact streaming framing. Consume at most the permitted bytes plus one lookahead; bound allocation by validated dimensions. Share the existing pixel validation rather than introducing a second parser.
- [ ] Run `cargo test -p board-media --locked` and scoped clippy. Commit only owned files. Review the task against the spec and real test evidence.

## Task 2: Guest and runner

Files: new guest crate, `scripts/media/` for build, provision and fixed runner, `tests/media/` for benign guest probes and actual containment assertions; adjust workspace only if needed.

- [ ] Write an actual local VM test requiring a known synthetic pixel result and absence of job artifacts/service after return. Run it before the runner exists and record the missing behavior.
- [ ] Build a minimal guest that reads its raw input device, runs an unprivileged bounded decoder and writes the fixed output device. Pin dependencies and keep parser code outside public/staff applications.
- [ ] Implement a fixed operator runner using a new jail and generated job ID per invocation. Fail closed on absent runtime/hash/configuration/resource support. Start the VMM through externally enforced limits. Never accept worker command/path metadata.
- [ ] Test valid decode, invalid input/output, timeout, actual resource enforcement and full cleanup with bounded harmless jobs. Test filesystem, credential and connectivity restrictions from the actual worker identity, with healthy allowed controls.

## Task 3: Integration and evidence

- [ ] Document exact local scope, hashes, commands, failures and unrun production checks. Update media/readiness/compatibility and dependency records.
- [ ] Run affected workspace tests, formatting, clippy and applicable CI checks. Review the whole branch and address findings.
- [ ] Commit and push a reviewable checkpoint with a draft PR tied to issue #5. Preserve the separate PR #17; do not merge it.
- [ ] Continue toward authenticated queue integration, fenced publication, compatibility and production qualification; do not treat the local runner checkpoint as completion of the overall rewrite.
