# Private classifier diagnostics

Migration 0199 adds a per-attempt diagnostic ledger for the late posting checks.
It is separate from the two-row-per-session cooldown history. Diagnostic rows
never authorize a refusal, extend a cooldown or grant permission to post.
The posting handler is not yet connected to this recorder.

## Recorded data

The ledger accepts the twelve active prefix event kinds and the four edge/country
history event kinds. A plan contains at most three events: an optional leading
`log_mask_changed`, followed by one stopping event or the ordered pair
`country_changed`, `block_country_changed`. A mask-change log can also stand
alone. Duplicate and impossible combinations are rejected. Database-assigned IDs
preserve the plan's order, including when several events share a timestamp.

Each row contains a database timestamp, kind, board and scalar thread ID. Zero
means a new-thread attempt. There is no post/thread foreign key: an unsuccessful
attempt may have no content row, and later content deletion must not remove the
attempt's diagnostic record. Country is retained only for `country_changed`.
A finite nonnegative model threat score can accompany events whose source log
included it, including the fresh `block_bm_bot` event. Missing optional metadata
stays absent.

Raw IP addresses, session identifiers, credentials, cookies, headers, comments,
filenames and request signatures are excluded. The source's formatted metadata
can include a bearer capability; it is not copied. The model threat score is not
the separate ingress bot score. The current ledger omits that ingress value and
the source's free-form metadata. These are deliberate backend diagnostic/privacy
differences, not claims of byte-for-byte event payload parity.

Public and staff database roles cannot read the table or use its sequence. The
public posting runtime can call only the bounded recorder on a locked,
CAPTCHA-enabled public board. The typed Rust interface accepts event variants,
not arbitrary names. Catalog checks cover the table, sequence, constraints,
indexes, helper definitions and grants.

## Transactions and capacity

The recorder uses its caller's transaction. A late refusal must first discard
provisional content, then replay any earlier originality effects and write the
refusal and diagnostics in the transaction that will commit. A failed commit
must not refresh an anonymous session cookie. Database tests cover combined
rollback and commit without creating a post or advancing posting activity.

The ledger is capped at 100,000 rows through the runtime writer. Event writes
serialize while checking capacity and inserting a batch. Each write first removes
at most 1,000 expired rows. If the complete batch still cannot fit, it returns
`CapacityDropped` and adds no diagnostic rows. This result must not change the
selected posting outcome: a refusal remains a refusal, and a nonblocking
mask-change log does not become a posting restriction. Schema or authority drift
is an error, not a capacity result.

## Retention

Rows strictly older than seven days are eligible for deletion, matching the
source event-log age purge. Insertion-time pruning is only a fallback. Run
`board-classifier-retention run` every minute to handle quiet periods and
backlogs. Each transaction removes at most 1,000 rows; one invocation performs
at most sixteen transactions. The cutoff comes from the database clock.

The command uses the existing `board_moderation_retention` login through
`MODERATION_RETENTION_DATABASE_URL`. It refuses migration credentials. The login
has no diagnostic table access; the expiry helper checks its nonprivileged role
posture, absence of role memberships and the classifier catalog. Only a real
retention or migration session can invoke expiry; changing the active role does
not impersonate either session.

1. Apply all migrations through 0199 with the usual migration account.
2. Reuse the dedicated retention login described in
   [moderation snapshot retention](../moderation-snapshot-retention.md). Give it
   CONNECT to the intended database, with no role memberships.
3. Build `cargo build -p board-store --bin board-classifier-retention --locked`.
4. Run the command locally with the private connection setting, then schedule it
   once per minute. The supplied `deploy/classifier-retention.service` and timer
   use `/opt/paperboard/bin/board-classifier-retention` and the existing protected
   `/etc/paperboard/moderation-retention.env` file. This is a separate service;
   classifier drift does not stop moderation snapshot expiry.

The worker prints counts and a batch-limit flag, never row contents or connection
settings. Scheduling delays, locks and downtime can postpone physical deletion.
Investigate failed runs and repeated batch-limit results, and drain expired rows
before inspecting a restored database. Backups and exported copies need their own
retention policy; deleting database rows is not secure disk erasure.

## Verification

`classifier_diagnostics` tests runtime permissions, ordered plans, rollback,
invalid plans, catalog drift, bounded expiry and concurrent writers at the exact
capacity boundary. The Robot9000 unit fixtures also combine country history,
diagnostics and originality replay in one transaction.

`scripts/test-classifier-diagnostics-migration.sh` checks a populated predecessor,
rollback, hostile default privileges, helper/role drift, the real retention
command and populated dump/restore. Run it only in its owned disposable test
cluster after building the retention binary.
