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
