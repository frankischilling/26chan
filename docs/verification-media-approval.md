# Durable media approval verification

September 9, 2026, on `feature/media-approval`, based on orphan recovery commit `5087d2ec520792bda49b8581b2b41dd658c7a9ee`. The working directory remains `C:\Users\imike\4chan-rewrite` and origin remains `frankischilling/26chan`. All commits preserve the configured Francis Hagan identity. Public media remains disabled; this checkpoint does not establish complete compatibility or production readiness.

The [approval contract](media-approval.md) separates database approval from the existence of a file. Each lease reserves a separate output ID and exact host-generated PNG metadata. Publication and reconciliation share one nonblocking filesystem lock. The reader uses a separate approved-only database view before reading bounded bytes. Approved records survive terminal queue cleanup. The database migration leaves legacy receipts unapproved.

## Local checks

Environment: Windows, Rust 1.94.0, PostgreSQL 16.15 in the disposable WSL Ubuntu cluster on port 55432. Browser versions remain Playwright 1.62.0 / Chromium 151.0.7922.34. Database tests source the ignored database, media, media-reader and staff credential files; spawned runtimes receive only their own credentials. No credential values are included in this record.

| Check | Observed result |
| --- | --- |
| Reader bootstrap and owner migration | Created the separate `board_media_read` login; migration 0008 applied successfully. Re-running bootstrap refused to replace credentials. |
| `scripts/test-role-bootstrap.sh` | All migrations applied from the staging role template in a separate private Unix-socket-only PostgreSQL cluster. Reader remained NOLOGIN with approved-only grants; the cluster was stopped and removed. |
| `cargo test -p board-store --features database-tests --test media_assets --test media_queue --locked` | Three approval and two queue tests passed, including actual role denials, concurrent approval, expiry while waiting on a row lock, bounded cleanup, immutability and approval survival. |
| `cargo test -p board-media --test publication --locked` | Four Windows tests passed, including contention from a separate process and lock release after process termination. The Unix symlink case runs in Linux CI. |
| `cargo test -p board-config --test media_reader --locked` | Configuration subprocess cases passed for valid reader, wrong role/host/mode and inherited credentials; public and writer configuration reject the reader secret. |
| `cargo test -p board-media-admin --test approval --features database-tests --locked` | Integrated database/filesystem/subprocess test passed. Pending files stayed unreadable, expired publication failed, retry IDs differed, four interruption windows reconciled, and approved reads survived queue deletion. Corrupt and missing files were denied. |
| `cargo test --workspace --all-features --locked` | Passed with no failures or ignored tests. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed. |
| `cargo fmt --all -- --check` and workspace bins/examples build | Passed. |
| Public and staff browser suites | Five public behavior tests and one WebAuthn/moderation scenario passed. |
| Windows visual suite | Three existing screenshot baselines passed; none were regenerated. |
| `scripts/test-media-approval-migration.sh` | Passed: 0007 queue data preserved, no automatic legacy approval, new reader sees only approved rows and cannot read the base table. |
| `scripts/restore-exercise.sh` | Passed: post and asset fingerprints, fifteen table counts, approved-only view and restored runtime denials. Both pending and approved synthetic asset records were included. |
| `actionlint` and changed shell-script syntax | Passed. |

Tests were added before the implementation. Initial failures demonstrated missing schema, database APIs, storage APIs, reader settings and operator binaries. The controller's first local VM harness attempts stopped before intake because a shell alias was unavailable and WSL path quoting was incorrect. Explicit script execution and forward-slash path conversion resolved those harness issues.

## Actual VM handoff

An owned WSL VM used the existing reviewed `/tmp/26chan-media-repro-20260909/decode.json` artifacts. Native Windows commands performed private intake and claimed a lease into a private manifest. The root runner received a cleared environment and only that job's input plus a stopped-output destination. The Windows publisher validated the stopped disk, installed its host-generated PNG and committed approval. The separate reader returned identical bytes both before and after deletion of that fixture's terminal queue row. Its private lease manifest was then removed.

This demonstrates an operator-mediated handoff across the existing VM profile. It does not establish authenticated service dispatch or Windows power-loss durability. The native Linux CI harness `scripts/test-media-publication.sh` repeats the complete flow alongside the existing VM and recovery suites. Hosted run results are attached to the draft pull request; they must be checked for its current commit before integration.

At commit `1b575e46788813011318a1da417e4c504b17722e`, the [Linux and Windows workflow](https://github.com/frankischilling/26chan/actions/runs/34386483871) and [advisory workflow](https://github.com/frankischilling/26chan/actions/runs/34386483854) passed. Linux ran 127 Rust tests without failures or ignored tests, five public browser scenarios, one staff scenario, five cgroup fixtures, eight VM tests and nine recovery tests. The actual VM publication harness passed, including the approved read after queue deletion, followed by the migration and asset-aware restore exercises. The runner used kernel `6.17.0-1022-azure`, systemd 255 and cgroup v2. Windows passed all three existing visual baselines.

## Review and limits

A separate read-only review approved database commit `067a955b0efdcd7a632dafea85c09a9e537c7996` with no Critical or Important findings. It assessed the recorded tests without rerunning them. Storage and whole-branch review records accompany the draft pull request; they do not substitute for review of a deployed processing tier.

Storage review identified one Important configuration issue: SQLx URL options could change connection settings after validation inspected only the URL authority. Writer and reader development settings now reject all query strings and fragments. Validation-only regression cases first failed, then passed after the fix; no external connection or credential transmission was used to test it. The local configuration suite and Clippy passed again.

Whole-branch review approved `9ecbdfe7c4463e2a7fa7c88782268f5ca34a95d6` for the documented development scope with no remaining Critical or Important findings. Its hosted advisory and both Windows visual checks passed. Initial Linux runs and their retries failed before application checks because the runner's Google Chrome stable APT index had a hash mismatch. CI now selects only Ubuntu sources for this disposable job, preserving package signature/hash verification; Playwright still supplies the pinned browser. A read-only local `apt-get --print-uris update` check verifies source selection, and hosted checks qualify the resulting environment. The runner's [APT setup](https://github.com/actions/runner-images/blob/main/images/ubuntu/scripts/build/configure-apt.sh) identifies the Ubuntu 24.04 source file; the [APT documentation](https://manpages.ubuntu.com/manpages/noble/man8/apt-get.8.html) specifies the source-list configuration keys.

A follow-up bootstrap inventory added the NOLOGIN reader to `deploy/roles.sql` and updated the getting-started recipe. Review caught the new bootstrap test calling cleanup twice on success: its output looked successful but the second cleanup returned exit 1. That status was reproduced explicitly, the completed cleanup now disables its EXIT trap, and the same fresh-cluster exercise returned exit 0. The failure trap remains installed until cleanup succeeds.

Fresh CI at `305935f54e84b94ed42dacaef1d3cc7c0424b052` then caught a separate development provisioning error: the role template now created `board_media_read` as NOLOGIN, while the development reader script only set an existing role's password. The actual reader integration test failed to authenticate. The earlier green workflow did not qualify this later bootstrap change. Development provisioning must explicitly enable LOGIN after its disposable-cluster and credential guards; staging keeps NOLOGIN until its operator configures access.

The corrected development script sets LOGIN with the generated password. A private mount namespace with its own `/tmp` and a separate Unix-socket-only PostgreSQL cluster exercised the actual script against the staging role template. The original script failed authentication; the corrected script authenticated with SCRAM using the generated credential. A second provisioning attempt refused, both credential files remained unchanged, and the original credential still authenticated. The fresh staging bootstrap test separately passed with the reader still NOLOGIN. Neither check changed the shared development cluster or its credentials. The existing fresh CI bootstrap and actual reader integration cover this transition on every Linux run.

The deliberate constraints are one canonical output root per database, operator-controlled directories, one permanent store lock, and Windows development-only durability. The database does not verify the configured filesystem root. Incorrect operator mapping can leave untracked files; a future storage migration must pause and reconcile publication. Serial publication limits throughput and may need a reviewed sharding design. Unix directory synchronization is implemented, but controlled storage/database power-loss tests remain outstanding.

Pending files share the trusted reader's directory: a compromised reader with filesystem access could read them, though normal reads require database approval. Worker containment does not rely on trusting the reader or granting the worker those paths. Public HTTP media, post attachments, authenticated dispatch, retention policy and production host/network/storage qualification remain required.
