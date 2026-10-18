# Configured restriction templates

The staff ban page can apply an operator-configured reason and duration preset.
Each choice shows its scope and duration, with public reasons available for review
before submission. The manual form remains available. Templates start empty: the supplied source
contains the selection logic, but not the private template records or their
production reasons and permission levels.

This covers preset issuance and its upload-history snapshot. Optional post-ban
actions and the rest of issue #220 remain open. Ordinary janitor request creation
and review use a separate [protected workflow](janitor-request-review.md);
private-media retention and blacklist effects are still pending.
The [prior-ban upload gate](admission/file-blocking.md#prior-ban-upload-gate) uses
these snapshots. Other admission work remains open under #212.

## Configuration

`restrictions.templates` is private operator configuration. Each row specifies
its ID, display name, minimum role, enabled state, board/global scope, warning
or ban duration, public/private reasons, whether a live file is required, and optional `source_rule` evidence.
Issuance and listing reject configurations with more than 128 rows. Reason and
duration limits match the existing restriction issuer.

A warning requires zero seconds. A finite ban requires positive seconds; a
non-warning with NULL seconds is indefinite. Source IDs 1, 123 and 213 require
a live file regardless of the display name (`admin.php:2802–2804`), so their
configuration must explicitly set `requires_file=true`. Other IDs can also
require a file. IDs are compatibility identifiers, not authority by themselves.

Use the migration/operator account to manage this table under
`board_restriction_owner`. Runtime credentials cannot write configuration or
read the table. Do not import a source template containing unimplemented
post-ban actions and assume those actions will run; this preset path currently
applies the restriction and its qualified automatic-range event, when configured.

The minimum role further restricts the existing moderator-and-above issuance
path. Configuring a janitor minimum does not enable direct janitor bans. That
request-and-approval workflow uses its own session-bound writers.

## Issuance authority

The selected template ID goes to a dedicated endpoint. Submitted reasons,
durations or scope cannot override its configured values. The database checks
the live account, session, CSRF proof, board permission and recent authentication,
then locks the selected enabled template. It reuses the existing post-restriction
issuer for target identity, actor locks, scope, duration, capacity and audit.

The ban page reads permitted template details in its existing authentication
transaction. This avoids contending with its own session lock on another
connection. The read-only wrapper lives in the authentication schema;
authentication credentials still cannot access the content schema or issue bans.
Listing does not lock the global restriction-capacity row.

A changed, disabled or removed template is checked again on submission. The
stored ban keeps the selected historical ID without a foreign key to mutable
configuration. Removing a preset does not rewrite its past restrictions.

## Upload observation and retention

The same transaction records the selected template ID and the target post's
retained normalized-upload digest. The observation is explicit:

- `absent`: the post has no attachment row
- `unknown`: an attachment exists but has no retained normalized digest
- `known`: the attachment supplies its captured 16-byte digest

File-rule IDs and original-input MD5 values cannot substitute for these facts.
Existing bans remain without template provenance. Existing attachments that
predate retained capture remain unknown; there is no inferred backfill.

File-only deletion and asset collection preserve the attachment's retained
digest, so a later authorized template ban can still snapshot it. Whole-post
content erasure clears the attachment copy. An already-issued ban keeps its
separate moderation-history snapshot, just as it retains its reason and audit.
Unban, expiry and later post deletion do not erase that history. A genuine
moderation-record purge would remove the ban row and its contribution together.

This distinction is required by the source upload-history query: it counts
retained ban rows by MD5 and template, without joining live posts or filtering
active bans (`lib/postfilter.php:2364–2395`). The original staff writer records
the post MD5 on the ban (`admin.php:2889`). Ordinary content deletion is not a
purge of restriction history.

Snapshot storage is charged with the ban. Failed template permission,
file-required or capacity checks roll back the base ban, audit and snapshot in
one transaction. Public status responses and media views do not expose the
private snapshot fields.

## Automatic-range events

`source_rule` is a private, optional string bounded to 255 bytes. Set it only
from the intended template rule. A non-warning template whose rule contains the
case-sensitive substring `global` invokes the mobile-range producer. The ban's
separate local/global scope is not a substitute for that predicate. Existing
presets keep NULL evidence after migration and acquire no automatic effect.

The producer uses the stored post's mobile browser comparison and keyed IPv4
prefix. It receives no caller-supplied network metadata. Ban and event commit
or roll back together. See [automatic-range policy](admission/automatic-range-policy.md)
for scope, deduplication and the unfinished janitor-request path.

## Janitor request configuration

Request creation requires an explicit `request_save_policy`: `none` or
`json_only`. Both retain the post presentation for review. Neither retains a
private media copy. On acceptance, `json_only` preserves related pending requests;
`none` uses the source's ordinary duplicate-clearing rules. The `everything`
value is reserved but currently rejects creation until private file retention is
available. It does not silently fall back to text-only evidence.

`request_can_warn` defaults to false. An explicit warning checkbox requires it,
even on a zero-day preset. Leaving the checkbox unset still creates a warning
from a zero-day preset. Async template submissions ignore that checkbox.

Review also requires `request_blacklist_policy='none'`. NULL means unconfigured.
The reserved `image` and `rejectimage` values currently reject acceptance rather
than omit a required file effect. Direct moderator preset issuance is unchanged
by these request-only fields.

For an existing template whose intended policy matches those limits, use an
operator transaction and substitute its reviewed ID:

```sql
BEGIN;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.templates
SET request_can_warn = true,
    request_save_policy = 'json_only',
    request_blacklist_policy = 'none'
WHERE id = :template_id;
COMMIT;
```

Do not assign these values to every imported template. Confirm its intended
retention, duplicate-clearing and blacklist behavior first.
