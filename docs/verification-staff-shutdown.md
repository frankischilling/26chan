# Staff shutdown verification

[Issue #44](https://github.com/frankischilling/26chan/issues/44) covers active
staff requests interrupted by the normal systemd SIGTERM stop signal. The
regression and fix are in [PR #45](https://github.com/frankischilling/26chan/pull/45),
which records final hosted checks, reviewed revision and merge status. The base
is main `073d3e986eb9ed472bbeb5faae808530c92c2fcd`.

## Before the fix

Regression-only commit `2c3a3b813ab4dcd5795321ad7057db81c54c5ce0` left the
production binary unchanged. The [Linux push run](https://github.com/frankischilling/26chan/actions/runs/34518953473)
passed formatting, warning-denying Clippy and compilation, then failed inside
`cargo test --workspace --all-features --locked` on September 10, 2026:

- `sigint_drains_active_staff_request_and_closes_both_listeners` passed.
- `sigterm_drains_active_staff_request_and_closes_both_listeners` failed at
  the assertion that the staff process must remain alive while a request drains.
  The actual failure was `signal terminated staff before draining its active request`.
- The shutdown suite reported one passed, one failed, none ignored. This expected
  failure stopped the workflow before its later browser, media and recovery steps;
  it is not a passing application run.

Both Windows visual jobs passed for this regression-only checkpoint. Unix
signals are not exercised by those jobs. Local WSL remained unresponsive to a
read-only `uname -r` command, so native evidence comes from hosted Linux.

## What the regression proves

`apps/staff/tests/shutdown.rs` runs the actual compiled staff binary with only
its dedicated database credentials and fixed development origins. A successful
`/readyz` checks both real stores. The fixture sends a small incomplete login
body, observes exactly one active handler through authenticated metrics, then
sends SIGTERM or SIGINT to its owned child PID. It requires the process to stay
alive while application admission closes and metrics remain available. Completing
the body must produce the entire expected HTTP 401 response before successful
process exit and closure of both listeners.

Socket operations and polling have deadlines. A child guard terminates and reaps
the owned process on assertion failure. No accounts, sessions or posting records
are created, no database locks are held, and no test-only route is added. The
production fix only registers Unix SIGTERM alongside the existing Ctrl+C path;
both original regression tests remain unchanged.

Run against the migrated disposable development database:

```sh
source .local/staff.env
cargo test -p board-staff --features database-tests --test shutdown --locked
```

CI includes this suite in `scripts/verify.sh` through its all-features workspace
test command. The PR separates the failing regression checkpoint from the fixed
revision's results and source review. Local formatting or source review cannot
substitute for those native results.

This checks a development process on an owned Linux runner, not a deployed staff
systemd identity, network policy, production authenticator or forced-stop policy.
It does not establish response-write deadlines or graceful handling of SIGKILL.
