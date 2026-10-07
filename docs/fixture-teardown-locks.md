# Fixture teardown and parent locks

At `38e653a`, the [Linux job](https://github.com/frankischilling/26chan/actions/runs/37550690150/job/112565173291)
failed in `user_thread_quota` cleanup, after the rejected-attachment quota checks.
Deleting a private deletion-secret row hit SQLSTATE `55P03`: its guard could not
obtain the parent board lock with `NOWAIT`. Eight other cases passed.

The writer locks the board before checking the quota. An early error drops its
SQLx transaction, which queues rollback rather than awaiting completion. The
old teardown issued independent autocommit deletes immediately afterward. This
is a source-supported explanation of the race; the hosted failure did not
record the identity of the blocking backend.

Fixture cleanup now begins one owned transaction, locks its boards in slug
order and their threads in board/ID order, then retains those locks through all
cleanup deletes and commit. The private guard remains enabled and unchanged.
A shared helper also covers the closely matching cleanup paths in
`automatic_admission_identity`, `janitor_posting_cooldown` and
`anonymous_session`. Those three were latent source risks, not independently
reproduced CI failures.

A deterministic regression holds a board or thread lock in a separate
transaction. It observes the exact cleanup backend and parent-lock query through
`pg_blocking_pids`, checks unchanged fixture state while cleanup waits, releases
the holder, and verifies that the owned rows are removed. It does not infer
ordering from a sleep or relax runtime timeouts.

Local PostgreSQL qualification passed all ten quota cases once serially and in
three ten-thread runs, plus 55 neighboring identity, session, archive and report
cases. Fixture mutexes isolate independent cases; explicit races within cases
remain. Formatting and strict workspace Clippy passed. Hosted results must be
checked on the published commit; these local checks do not certify full parity.

## Anonymous session parents

Build 37580030054 on `d2f94c2` failed in anonymous-session registration with
SQLSTATE `40P01`. The expiry sweep held a session parent and waited to cascade
into `anonymous_posts`. Fixture teardown could hold that membership through a
hard post delete, then request the same session parent. The hosted log contains
the sweep's statement and reciprocal waits, but not the other backend's exact
statement or test identity.

This reverse ordering is present in test cleanup. Production proof locking takes
the session before membership, and public deletion soft-deletes content. No
production hard-delete/session-delete inversion was identified, so runtime
expiry, timeouts and authorization are unchanged.

The owned cleanup helper now takes sorted session-parent locks after existing
board/thread locks and before child deletion. Anonymous-session and automatic
identity fixtures use it; report-group, group-lifetime and categorical-report
cleanup retain equivalent parent locks in their existing transaction. Only the
fixture's tokens are locked. No global mutex or retry is added.

The separate `anonymous_cleanup` integration binary contains one bounded
scenario with legacy and corrected controls. An observed policy-row gate stages
a real public mint. Legacy teardown produces the expected wait on its child
membership; releasing teardown lets the mint succeed. Corrected teardown lets
the mint complete while its protected session is skipped. A subsequent unlocked
mint must still retire that expired session and membership while preserving the
post and an unrelated live membership.

The deliberate global gate runs in its own test binary. Putting that probe among
independent parallel mints caused fixture interference, so those ordinary cases
remain parallel and the controlled scenario is isolated. The probe, all 14
anonymous-session cases and 31 neighboring identity/report cases pass locally.
All 226 store checks pass with eight test threads. The ordinary 14-case session
suite also passes three additional eight-thread runs. Formatting and strict
workspace Clippy pass. Exact-head hosted qualification remains pending.
