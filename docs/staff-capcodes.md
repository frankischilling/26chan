# Staff posting and public badges

The staff application creates text threads and replies with the six badge
values in the pinned public API: `mod`, `admin`, `admin_highlight`, `manager`,
`developer` and `founder`. A saved badge is public display metadata. Requests
still require a current staff session and its posting permissions.

## Posting and permissions

Sign in at the configured staff origin, open Staff posting from the report
queue, select the board and badge, then enter a thread ID or `0` for a new thread. The form
works without JavaScript. Posting requires the current session and CSRF proof,
same-origin request metadata and authentication less than ten minutes old.
Successful submission links to the persisted public post.

Badge choices follow the source `parse_capcode` and global flag checks:

- Janitors cannot create public posts with a staff badge.
- Moderators need the global `capcode` flag for `mod`. The global `developer`
  flag permits `developer` independently of `capcode`.
- Managers can choose `mod` and `manager`; `developer` requires its global flag.
- Administrators can also choose `admin`, `admin_highlight` and `founder`.
  They need the global `developer` flag for that badge too.

These particular source flag checks require an allow for `all` or the literal
empty board, and no `noboard` deny. Access to a single named board does not
grant global flag access. The selected board must also be allowed and not
denied. Both proof issuance and consumption recheck these permissions,
including after row-lock waits.

The operator's configured label remains the form's default when eligible.
Moderators default to `mod`, managers to `manager` and administrators to
`admin`. Set a default or explicit posting flags through the offline operator:

```powershell
target/debug/staff-operator.exe capcode alice default
target/debug/staff-operator.exe flags alice capcode,capcodename
```

The default label grants no extra permission. Operator assignment, flag and
scope changes invalidate sessions. A role change also clears the explicit
default label. Historical posts retain their saved identities and badges.

## Names, tripcodes and board policy

Public staff names use whole-field cleanup, CP932 legacy trip preparation,
display-name cleanup and the source finished escaped name/trip bound.
[Rank-specific limits](verification-authorized-post-limits.md) permit 255-byte
raw fields and each board's authorized comment budget for moderators and
higher ranks. The finished escaped name and generated trip wrapper together
must fit 255 bytes. That check occurs before capcode name masking.

Without the global `capcodename` flag, a non-administrator's name becomes
`Anonymous` and its prepared trip is removed. Administrators retain their
prepared identity without that flag. Forced-anonymous boards clear every
subject and clear non-administrator names; administrator names and trips
remain eligible. Board trip suppression still removes administrator trips.

Legacy trips require no secret configuration. To enable secure staff trips,
set `STAFF_TRIPCODE_KEY` to the same private 64-digit hexadecimal key used as
the public application's `TRIPCODE_KEY`. Keep it stable across restarts. The
modern secure hash replacement is described in [tripcodes](post-identities.md).
Only the prepared display hash is saved or rendered. Raw trip passwords stay
out of posts, proof payloads and public responses. Unrelated public, media and
observer runtimes reject the staff-specific key; the media job runner rejects
it before configuration or execution.

The shared posting transaction retains board subject, comment, forced-anonymous,
closed-thread, bump, reply-limit and rollover rules. Staff posts have no public
deletion password and use moderation for removal. Badged staff posts on boards
with IDs save the [source static label](source-staff-poster-ids.md). They save
no geographic flag, board flag or private network fingerprint. A thread containing a staff
post therefore cannot claim a complete private poster count.

Private discussion keeps its separate anonymous role/alias presentation and
ordinary janitor limits. This flow accepts text only. Staff attachment
posting, unbadged authorized posting, Pass/VIP behavior and other privileged
source exceptions remain unfinished. [The authority and peer-transport
continuation](staff-posting-authority.md) records the ordinary-post
Robot9000 distinction and the verified staff proxy preparation. The public
attachment owner has no staff authentication access or badge authority.

## Database boundary and upgrade

Migration 0042 creates a separate `board_staff_post_owner` NOLOGIN function
owner. Fresh installations receive it through `deploy/roles.sql`. Existing
installations need the bootstrap administrator to apply
`deploy/staff-post-role.sql` before running the migration identity. Apply
migration 0071 with staff instances stopped, then start updated binaries and
check `/readyz`. Startup requires the new source issuer and restricted grants.

The migration preserves historical rows, account flags, default labels, old
proof payloads, function OIDs and grants. It adds source choice, prepared trip
and name-permission fields to new proofs. No account receives new flags
automatically. Older pending named proofs can be consumed only while their
accounts still have the source badge and name permissions. Older issuers
cannot authorize trips through transaction settings.

The authentication role can issue a 15-second, single-use authorization tied to
one current session, CSRF hash, selected badge, name permission and every
prepared field: number, board, thread, name, trip, subject, comment, formatter
data and timestamp. It permits at most
32 active authorizations per account and removes expired records in bounded
batches. Only the content role can consume the authorization. The invoker
insertion trigger checks that role and the exact payload, rechecks session and
account authority after row-lock waits, consumes the record and writes the audit
entry in the post transaction. A rollback leaves neither post nor audit committed.

Neither runtime can read or edit the private authorization table. The scoped
owner can read credential IDs and their account ownership, but no passkey
documents. It cannot change account roles, flags, default labels or expiry clocks,
insert content, manage deployments or access media. Its column update grants
permit the required row locks and session-activity refresh. Board reads are
limited to slug, ordinary/authorized comment budgets, forced anonymity and
trip suppression. The private badge/name helpers have no runtime execute
grants. The public runtime cannot mint, consume or set
badges through SQL, forms, cookies or headers.

## Public rendering and evidence

Full thread, index and catalog JSON include saved `capcode` and eligible
prepared `trip` values. The [source JSON projection](source-staff-json.md)
applies its separate name/trip mask and meta-board reply groups. Tail replies
use that projection; the tail's minimal OP metadata omits identity fields.
Both listeners use the same representation. Pages, updater snapshots and quote previews render the fixed
name classes, labels, highlight groups and local identity icons. Click and
keyboard controls select the corresponding staff group. Founder joins the
administrator group, as in the pinned client. Staff labels have no ID-count
tooltip. The moderation queue displays escaped badge and flag text. Desktop
and mobile pages, updater snapshots, quote previews and the queue retain the
prepared trip beside the badge. On forced-anonymous or meta boards, catalog
hover details and identity filters retain names and trips only for saved
`admin` and `admin_highlight` badges. The catalog's saved-badge rule is narrower
than posting's administrator-rank exception. Founder and other badges remain
anonymous in those catalog details.
The meta-board switch is independent of private board access.

The [reference manifest](public-capcode-reference.json) pins the API revision,
released client, six public stylesheets and nine identity GIFs. Their paths are
listed individually in the image CSP. Founder uses its captured 16-pixel icon.
Badge titles refer to this board's staff. These namespace and credential
boundaries are E-016.
The private source supplies the rank and board-scope behavior described in [staff operation](staff.md). Staff attachment posting remains unfinished.

The finite native recipe requires one complete badge in the post's own header,
matching label, classes, title, icon, density path and dimensions. An optional
trip must match the prepared hash grammar, fit the finished display bound,
occupy its own header position and agree between desktop and mobile. It rejects
badge or trip markup in comments, inconsistent identities, arbitrary images and
attributes before constructing DOM elements. Local copies preserve the badge
while omitting attached controls. Actual checks are recorded in
[staff identity verification](verification-staff-identity.md) and the earlier
[staff posting verification](verification-staff-posting.md).
