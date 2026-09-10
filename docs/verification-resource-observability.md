# Resource observability verification

Work for [issue #40](https://github.com/frankischilling/26chan/issues/40) starts
from main `3649d15d997d2cb9e9184c444a44d22b0346e9c6`; design/plan commit `7bb3445`.
Implementation and Linux qualification are in progress. No production deployment
or native pressure success is claimed by this record yet.

Portable checks on Windows, September 10, 2026:

- The strict configuration and collector parsers first failed against stubs.
  The completed crate passed 16 tests, including arbitrary input parsing,
  environment rejection, path validation, cache age/failure/recovery and bounded
  blocking admission. All-target Clippy passed with warnings denied.
- Independent runtime review found a late ready result could be accepted after
  its deadline. The regression failed against that implementation, then passed
  with an elapsed monotonic deadline check. A separate review finding led to
  channel-gated recovery testing so a busy runner cannot miss the failure state.
- Linux-musl library typechecking passed without linking or WSL execution.
  Native syscall behavior and actual mount/cgroup pressure await hosted CI.

Independent parent checks passed all 28 board-observe tests, all 20 resource-rule
cases and both existing rule suites, native validation of all 18 rules, ten
monitoring helper tests, and ten profile tests (one POSIX-only skip on Windows).
Workspace formatting and diff whitespace checks passed. The lockfile changes
only the new local package. Separate read-only review found no further blocker
in configuration/collection after verifying the sampler corrections. Independent
integration review found no additional blocker in the helper, service unit or CI;
two stale operations descriptions were corrected. The Linux helper must prove protected payload
and cgroup-control denials, writer/observer read-only agreement, real alert
transitions, and normal/SIGTERM cleanup before this slice can be merged.

Initial implementation `161945f` passed hosted Windows visuals and the advisory
scan. Linux CI stopped at `clippy::non_octal_unix_permissions` in a Linux-only
test. The permission value was changed from `0` to `0o0`; its value and assertions
are unchanged. Native pressure qualification had not run at that point.

Local WSL remains unresponsive; restart approval is pending. Existing WSL
processes were retained rather than replacing live tests. The full rewrite,
production resource/network/storage qualification and update-check alert delivery
remain incomplete. See [operations and measurement limits](resource-observability.md).
