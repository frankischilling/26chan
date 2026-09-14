# Protected-thread image admission

[Issue #126](https://github.com/frankischilling/26chan/issues/126) implements
the supplied `imgboard.php:5045-5052` image-count exception. On the ordinary
image path, the source checks the reply-image count only when the thread is
neither sticky nor undead. Permaage and permasage do not independently exempt
it. The count excludes the OP, deleted replies and deleted files.

This differs from the [public image-limit indicators](source-image-limits.md).
Permaage hides the indicator but still has the count cap unless sticky or
undead is also set. HTML catalog indicators have their own predicate and may
show a reached limit on an undead thread that can still accept images.

Migration 0026 reads sticky/undead while locking the thread inside the
attachment insertion function. Board, thread and job locks retain their
existing order. A flag change committed while posting waits therefore
controls that insertion. Only these two read grants are added to the
non-login attachment owner; it receives no authority to change the flags.
Public input fields and capcode text cannot establish either state.

The exception bypasses only the image-count comparison. It does not bypass
closed/archive state, approved output, capability authentication, expiry,
single-use records, attachment-only constraints or transactional rollback.
The source's separately privileged posting bypass is not granted through a
public request. Zero `image_limit` remains this deployment's explicit
disabled-media setting, including on protected threads. That fail-closed
setting is a security constraint, not source-equivalent unlimited images.
Media must still be enabled and its processing boundary qualified.

## Upgrade and rollback

Apply 0026 after 0025 and before accepting posts under the corrected rule.
It replaces the ten-argument function while preserving ownership, fixed
search path and execute grants. The nine-argument compatibility wrapper
calls that same function, so both old and current binaries receive the
corrected count exception. Historical posts, attachments, timestamps and
private OP records are not rewritten. Binary rollback does not undo this
database admission change; retain the additive migration and record that
behavior in the rollback decision. No new media parser or worker authority
is introduced.

## Verification

The actual-public-role attachment test covers all 16 combinations of sticky,
undead, permaage and permasage, with ordinary comments and attachment-only
posts. It checks counters and rollback, single-use receipts, disabled media,
closed state and expired capabilities. Two lock-wait cases observe a healthy
public backend blocked by the owned board lock before an operator commits
an exemption change. The final-slot race retains exactly one winner on a
permaage-only thread, which hides the indicator but is not exempt.

The HTTP regression uses approved metadata fixtures through both aliases
and form encodings. It checks persisted over-limit sticky/undead attachments,
permaage and ordinary rejection, unchanged counters/clocks on failure,
negotiated JSON errors and forged public flag fields. Its bounded synthetic
peer budget is 60 writes; production throttling remains separately tested.
Metadata fixtures are not decoder or publication-boundary evidence.

`scripts/test-image-admission-migration.sh` upgrades a populated disposable
database from 0025, checks historical retention and actual public logins,
and exercises both attachment entry points, read-only flag grants, permaage
rejection, closed state, expiry, single-use and rollback. Local all-target
Clippy and Bash syntax checks pass. PostgreSQL is unavailable locally;
complete current-head Linux CI is required before merge. Full reply admission,
large-thread reading, other cooldowns and deployed production qualification
remain unfinished.
