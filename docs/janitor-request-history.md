# Request history summaries

The protected request queue shows the target's retained ban and warning summary
and the selected template's public reason. The summary is attached to an
authorized request; there is no browser-supplied actor key or separate arbitrary
history lookup.

## Source arithmetic

The supplied `get_bans_summary` counts every retained matching record in its
total. Recent counts consider records created in the last 31,536,000 seconds,
including the boundary. Indefinite records contribute to the total but are
excluded from recent counts, active arrays and days.

A finite record lasting at most ten seconds counts as a warning, independently
of its native warning flag. The source's warning indicator includes records
created between one hour and one day ago, with both boundaries included. It
does not use the native viewed/retired state to remove a warning from that list.

For a longer restriction, days are based on time actually spent banned: an
explicit unban is capped at the original duration, a still-future expiry uses
elapsed time, and an expired restriction uses its full duration. The source
sums durations, including overlaps, before rounding up to whole days. Active
finite restrictions contribute their template IDs; an explicitly unbanned
record does not. The native reader uses whole-second timestamps and preserves
these decisions.

`fixtures/janitor-history-source.json` contains only the isolated arithmetic.
Its PHP runner uses synthetic timestamps and rows, without the original SQL or
application. The SQL policy is checked against the resulting source cases.

## Authority and bounds

Migration 0213 adds a private summary helper and an actor/ID index. The queue
retains its session, CSRF, role and board-scope checks. Helpers are not executable
by public, staff or authentication runtime roles. Only the existing scoped
security-definer queue calls them. The result exposes counts and available
template names, never actor hashes, addresses, private reasons or another
account's session data.

Each actor lookup reads at most 10,001 rows and refuses a history larger than
10,000. A page summarizes each distinct actor once, for at most 25 requests.
Repeated requests for the same actor share that page's summary and timestamp.
Template names remain limited by the queue's existing role-filtered choices.
An unknown or unavailable template still contributes to the count without
exposing its name.

Migration 0223 adds the retained Pass identity captured from the source post.
Request capture preserves it before source-post deletion, and approval copies it
into the restriction. Summaries match the retained address or Pass identity and
count overlapping matches once. The 10,000-row bound applies to that union.
The total remains labeled retained because original production identities and
unretained historical records are unavailable. This is not a complete staff-history
search interface.

## Template reasons

Public reasons are escaped as text in the queue, initial amendment and inert
shared template metadata. The shared metadata avoids duplicating every reason
in every request's dropdown. Changing a template restores that template's public
reason while preserving private notes. Hostile markup remains literal text.

## Verification

```sh
python3 scripts/extract-janitor-history-source.py /path/to/4chan-old --check
php -n scripts/test-janitor-history-source.php
REQUEST_FILE_EFFECTS=1 bash scripts/test-janitor-request-review.sh
cargo test -p board-staff --features database-tests --test janitor_requests --locked
node --test tests/browser/janitor-queue-state.test.mjs
```

The migration suite exercises rollback, helper privilege denial, scoped
metadata, size refusal, source arithmetic, review transactions and populated
restore. The HTTP/browser fixture seeds owned history and checks its rendered
counts, template reasons and absence of private notes. Existing expired-session,
revocation, wrong-origin and wrong-board controls remain in the suite.

Issue #223 remains open for full history search and source visual parity. Private retained media and thumbnail controls are covered
by PR #302.


## Request-linked history reader

The source queue links to a separate staff ban-search service using the target's
address or a Pass reference. That service's implementation is absent from the
supplied tree. The native reader uses a pending request ID to resolve the saved
address and optional Pass identity inside the database. It displays retained ban
and warning rows,
public reasons and private notes from boards the reviewer can access. Global
bans originating on an inaccessible board are also excluded.

Only moderators and higher roles can open this reader. Authentication, CSRF
cookie, source-board access and catalog integrity are checked for every page.
Once the request is reviewed or removed, its history URL stops working. The
reader accepts no address, password, actor hash or arbitrary account selector.
It provides 25 records per page in descending ID order, with an exclusive older-ID
cursor and a 10,000-row actor bound. New records do not shift an older page.
Rendered reasons and notes are escaped; responses remain private and uncached.

This restores the queue's retained-identity ban lookup, including Pass-linked bans
from other addresses. Post searches by address, password and file hash, and the
separate staff service's layout remain outside this reader. Browser coverage exercises the link and
repeated newest-page navigation on desktop/mobile with scripts on and off.
SQL tests cover pagination, cross-board and unrelated identities, role refusal,
changed scope and removed requests. HTTP checks cover authentication, cache
headers and stale URLs.


The Pass lifecycle SQL fixture verifies a same-address/Pass overlap, a matching
Pass from another address, an inaccessible board and absence of raw identity
hashes in both summary and detailed output. The detailed reader retains its board
scope checks; summary counts do not reveal hidden-board reasons or private notes.
The displayed description now includes Pass matching rather than the obsolete
address-only limitation. See `scripts/test-pass-restrictions.sql` and
`docs/public-pass-lifecycle.md` for that qualification.
