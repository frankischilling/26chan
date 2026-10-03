# Private staff discussion

The staff listener serves `/j/`, its numbered `.php` index pages, `/j/thread/{id}`, and the source `/j/res/{id}.php` aliases. A separate staff session is required for every page and post. Public pages, JSON, RSS, search and direct queries through the public database role cannot read this board, including requests that carry a staff cookie.

The source reference is `4chan-old/plugins/broomcloset.php` and the `JANITOR_BOARD` branches in `imgboard.php`. Names and email options are hidden on the form and ignored on submission. Every stored post is named `Anonymous`; a submitted name or requested public badge cannot replace it. Authorship is recorded in a separate identity-schema table, readable by the authentication role but not the staff-content or public roles.

The displayed label follows the author's current rank: Janitor, Mod, Manager or Admin. Its color matches the source. Real account names and author IDs are never put in the HTML. An author without current board membership is displayed as an unlabelled anonymous poster. The source's assignment-tooltip branch is inactive in the pinned checkout; this implementation does not invent a tooltip or disclose the mapping.

Posting uses the same short-lived, single-use authorization proof as public staff posting. That proof binds the post body, thread, board and timestamp, checks recent authentication, and is consumed in the content transaction. Private authorship and the audit entry commit with the post. An explicit `/j/` denial blocks page access, posting and latest-post polling even for an administrator. The database also prevents changing `/j/` into a public board.

Index and thread pages use the shared snapshot readers and escaped comment renderer, with working quote links and the original second-resolution New York timestamps. Invalid submissions retain escaped subject and comment fields. Reading does not require recent reauthentication; posting does. The report queue links to discussion only when the session may access it.

[Rank-specific posting limits](verification-authorized-post-limits.md) keep
janitors at 100-byte raw fields and 2,000 normalized comment scalars on `/j/`.
Moderators, managers and administrators receive 255-byte fields and 50,000
comment scalars, with the source's repeated-line and line-cap exemptions.
The proof rechecks rank and board policy before saving the post. Names remain
anonymous at every rank.

Migrations 0060 and 0061 install the author mapping and grant the staff-content role access to the existing bounded poster-count function used by shared snapshots. They do not grant access to poster fingerprints or addresses. Private page rendering retains the authorization connection while reading authorship, so a page also works with a one-connection authentication pool. Serialized response bodies share a 64 MiB process budget, retained until response data is released.

The current implementation covers text discussion. Private attachment upload and delivery are still pending, as is integration of the source's non-expiring history policy. Keep issue #221 open until those remaining board behaviors and their validation are complete.

With the owned database roles configured:

```sh
cargo test -p board-staff --features database-tests --test posting \
  private_discussion_forces_anonymous_roles_and_keeps_identity_off_public_routes -- --exact
```

The test exercises live PostgreSQL roles and real HTTP handlers. It checks all four rank labels, anonymous posting, source aliases, escaped output, quote links, author-table privileges, public leakage, explicit denials, forged authorship, CSRF, idle/absolute expiry, recent-authentication requirements, rejected-write rollback and bounded connection use.
