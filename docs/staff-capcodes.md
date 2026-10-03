# Staff posting and public badges

The staff application can create text threads and replies with the six badge
values in the pinned public API: `mod`, `admin`, `admin_highlight`, `manager`,
`developer` and `founder`. A saved badge is public display metadata. It grants
no request authority.

## Posting and assignment

Sign in at the configured staff origin, open Staff posting from the report
queue, select the board and enter a thread ID or `0` for a new thread. The form
works without JavaScript. Posting requires the current session and CSRF proof,
same-origin request metadata and authentication less than ten minutes old.
Successful submission links to the persisted public post.

Moderators default to `mod`, managers to `manager`, and administrators to `admin`. An offline
operator can assign a different allowed public label:

```powershell
target/debug/staff-operator.exe capcode alice manager
target/debug/staff-operator.exe capcode alice default
```

Only administrators can receive `admin`, `developer` or `founder`.
A manager can receive `mod`, `manager` or the role default; a moderator can receive `mod` or its default. Janitors cannot create public staff-badged posts. Assignment changes invalidate
all sessions. Changing a role clears its explicit public label and invalidates
sessions. Historical post badges retain their saved value. An administrator
assigned `admin` can choose the highlighted variant for one post; the other
labels cannot request that variant. Both authorization issuance and consumption check the current allowed and denied board lists; a board-scope change cannot leave an old authorization usable.

The shared posting transaction retains board subject, comment, forced-anonymous,
closed-thread, bump, reply-limit and rollover rules. Staff posts have no public
deletion password and use moderation for removal. The display name excludes any
private trip-password suffix. Staff posts save no trip, poster ID, geographic
flag, board flag or private network fingerprint. A thread containing a staff
post therefore cannot claim a complete private poster count.

This flow accepts text only. Staff attachment posting is unfinished; the public
attachment owner has no staff authentication access or badge authority.

[Rank-specific limits](verification-authorized-post-limits.md) allow 255-byte
raw fields and the configured authorized comment budget for current
moderators, managers and administrators. Janitors retain ordinary limits in
private discussion. Finished escaped display names still have a 255-byte bound.
Full source staff name preparation and other privileged exceptions remain open.

## Database boundary

Migration 0042 creates a separate `board_staff_post_owner` NOLOGIN function
owner. Fresh installations receive it through `deploy/roles.sql`. Existing
installations need the bootstrap administrator to apply
`deploy/staff-post-role.sql` before running the migration identity. Stop staff
instances for the migration, start the updated binaries and check `/readyz`.
Startup and readiness require the new public columns and function permissions.

The authentication role can issue a 15-second, single-use authorization tied to
one current session, CSRF hash, assigned label and every prepared post field:
number, board, thread, name, subject, comment and timestamp. It permits at most
32 active authorizations per account and removes expired records in bounded
batches. Only the content role can consume the authorization. The invoker
insertion trigger checks that role and the exact payload, rechecks session and
account authority after row-lock waits, consumes the record and writes the audit
entry in the post transaction. A rollback leaves neither post nor audit committed.

Neither runtime can read or edit the private authorization table. The scoped
owner can read credential IDs and their account ownership, but not credential key material. It cannot change account roles, badges or expiry clocks,
insert content, manage deployments or access media. Its column update grants
permit the required row locks and session-activity refresh. The public runtime cannot mint, consume or set
badges through SQL, forms, cookies or headers.

## Public rendering and evidence

Full thread, index and catalog JSON include the saved `capcode`. Tail replies
retain it; the tail's minimal OP metadata omits it. Both listeners use the same
representation. Pages, updater snapshots and quote previews render the fixed
name classes, labels, highlight groups and local identity icons. Click and
keyboard controls select the corresponding staff group. Founder joins the
administrator group, as in the pinned client. Staff labels have no ID-count
tooltip. The moderation queue displays escaped badge and flag text.

The [reference manifest](public-capcode-reference.json) pins the API revision,
released client, six public stylesheets and nine identity GIFs. Their paths are
listed individually in the image CSP. Founderâ€™s public `@2x` URL returned 404;
its available 16-pixel icon is retained without an invented density asset.
Badge titles refer to this boardâ€™s staff, avoiding a false claim of authority
on the reference service. These namespace and credential boundaries are E-016.
The private source supplies the rank and board-scope behavior described in [staff operation](staff.md). Staff attachment posting remains unfinished.

The finite native recipe requires one complete badge in the post's own header,
matching label, classes, title, icon, density path and dimensions. It rejects
badge markup in comments, inconsistent identities, arbitrary images and
attributes before constructing DOM elements. Local copies preserve the badge
while omitting attached controls. Actual checks are recorded in
[staff posting verification](verification-staff-posting.md).
