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

This covers retained address-identity history. The source also joins matching
Pass history from other addresses; that remains dependent on the unfinished
Pass identity lifecycle. The displayed total is therefore labeled retained,
not all-time. This is not a complete staff-history search interface.

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

Issue #223 remains open for retained media, thumbnail controls, full history
search, linked Pass evidence and source visual parity.
