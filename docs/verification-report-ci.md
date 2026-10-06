# Report checkpoint CI corrections

The [6fbed3e run](https://github.com/frankischilling/26chan/actions/runs/37468696466)
failed. Its evidence must not be replaced by the fully passing preceding
[f988a7a run](https://github.com/frankischilling/26chan/actions/runs/37460684026).

## Migration fixture permissions

The media job reached the new report-admission upgrade exercise and stopped at
line 33, before its assertions. The script creates a private SQL file as root
under umask 077, then asked a postgres-owned psql process to open that pathname.
The second generated readiness file had the same ownership mismatch.

Both calls now use `-f - < file`: the parent shell opens its private file before
switching the child user. File permissions are unchanged. The adapted local
exercise passes populated/fresh upgrades, historical preservation and current
dump/restore. That local environment does not reproduce the hosted user switch;
the original privileged CI harness still needs exact-head qualification.

## Attachment authority inventory

Linux passed preceding Rust targets, then stopped in `post_media` at its exact
attachment-owner function allowlist. Migration 0094 adds the staff-only
`staff_delete_post_attachment` wrapper, so the previous ten-function set was
stale.

The corrected test enumerates the reviewed functions and derives the expected
count from that set. It retains owner, SECURITY DEFINER, fixed search path and
PUBLIC-denial checks. A separate assertion requires the new wrapper to be
executable by staff, denied to public/auth roles, and not grantable to others.
It does not permit arbitrary new functions.

A broader local run also exposed a race in the archive-secret lock fixture:
the queued archiver could commit before its intermediate visibility assertion
following the rejected deletion. An explicit savepoint now keeps the original
secret lock until the test deliberately releases it. The required `55P03`,
pre-commit authority preservation and eventual retirement assertions remain.

## Windows image connection failure

Theme shard 1 passed 486 cases and failed the spoiler-image decode prerequisite
in `tests/themes/file-states.spec.js`. The trace recorded
`/static/catalog/spoiler.png` failing before HTTP with `net::ERR_NO_BUFFER_SPACE`.
The same asset returned 200 and 1,115 PNG bytes about 0.6 seconds earlier.

The retained NetLog links request 65834 through stream 65837 and connection job
65838 to socket 65839 at `127.0.0.1:3000`. Its TCP connect attempt returned Windows
error 10055 at 2026-10-06 13:18:06.385 UTC, followed by net error -176. This proves
a connection-setup failure for that request. It does not prove global port,
memory or nonpaged-pool exhaustion, or establish the underlying Windows cause.

The failed worker's diagnostic was retained. A later `discarded-success` message
belongs to the replacement successful worker. Main Windows visual assertions
passed; its final aggregate step failed because the required shard failed.
Assertions, image prerequisites, screenshot baselines, tolerances and retries
remain unchanged.

## Metadata-only diagnostic enrichment

AFD inventory schema 2 adds the provider GUID and deduplicated opcode, task,
validated log name and exact hexadecimal keyword metadata. Signed keyword
values retain all 64 bits rather than passing through JSON numbers. Numeric
channel ID and the raw event-descriptor keyword mask are unavailable through the
current collector and remain explicitly absent. Completeness does not guess
those values.

Capture remains unavailable. No ETW session, raw event collection, pointer
extraction, native decoder or additional system privilege is enabled. Existing
input/output caps, cleanup and original test exit-code handling remain. The 25
focused Node cases pass locally; PowerShell and hosted Windows behavior require
the next exact-head CI run.

## Anonymous upgrade ordering

The legacy anonymous-session exercise also applied every later migration before
0065. Replaying that script from 4fe5003 reproduced a missing
`post_secrets.anonymous_sessions` error at migration 0095. The corrected exercise
first verifies the populated 0064-to-0065 transition, then applies later
migrations in order and checks current runtime behavior. Its report fixture uses
the restricted admission API, and its complete dump uses the owned-cluster
administrator rather than expanding the migrator's private-table grants.
The adapted local exercise passes history, role-denial, rollback, capacity,
bounded-cleanup and restored-ownership checks.


## Session-report checkpoint prerequisites

At ea9a7b4, all four Windows theme shards and the main Windows job passed.
Linux reached the report-popup group with 14 passing cases and one Escape
failure: the popup closed during keydown before Playwright received its keyup
acknowledgement. The test now requires a trusted Escape event, popup closure,
an open parent, and a connected browser before accepting that specific
closed-target error. Other errors remain failures. Browser verification of
this correction is pending the next exact-head run.

The media job failed its complete dump because the non-inheriting migrator
cannot read the dedicated-owner report-admission tables. Both restore exercises
now dump through their already verified disposable-cluster administrator;
runtime grants are unchanged. Private files are opened before switching users,
and private report membership, admission policy, reports and audit history are
compared through silent fingerprints.

Both adapted local restore exercises passed with migration 0097. The local
adapter used an owned TCP PostgreSQL cluster and the current unprivileged user;
it does not qualify native root-to-postgres switching or Windows execution.
The attachment exercise verified seven attachments, fourteen files, exact JSON
bytes, URL forms, deletion/archive visibility, one-use receipts, interrupted
cleanup, numbering and unchanged source state.

A wider local run also found a transient empty media claim in the attachment
fixture. Its unchanged isolated rerun passed. The fixture now waits on its exact
queued job with a bounded row lock, verifies state, attempt count, input size and
expiry, then explicitly releases the lock before the single `SKIP LOCKED` claim.
This establishes the idle-queue precondition without retries or production queue
changes. The exact source of the transient lock has not been established.
