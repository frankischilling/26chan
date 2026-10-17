# Configured restriction templates

The staff ban page can apply an operator-configured reason and duration preset.
Each choice shows its scope and duration, with public reasons available for review
before submission. The manual form remains available. Templates start empty: the supplied source
contains the selection logic, but not the private template records or their
production reasons and permission levels.

This covers preset issuance and its upload-history snapshot. Optional post-ban
actions, the janitor request workflow and the rest of issue #220 remain open.
The prior-ban upload admission caller is also still pending under #212.

## Configuration

`restrictions.templates` is private operator configuration. Each row specifies
its ID, display name, minimum role, enabled state, board/global scope, warning
or ban duration, public/private reasons, and whether a live file is required.
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
applies only the restriction itself.

The minimum role further restricts the existing moderator-and-above issuance
path. Configuring a janitor minimum does not enable direct janitor bans. That
request-and-approval workflow remains separate work.

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
