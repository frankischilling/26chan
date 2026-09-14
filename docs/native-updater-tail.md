# Full and tail thread updates

The supplied `imgboard.php::get_json_tail_size`, `json.php` tail generation and
`js/extension.js::ThreadUpdater` establish this contract. Their file hashes and
version boundaries are recorded in the [source inventory](compatibility.md#source-inventory).
This work is tracked in [#90](https://github.com/frankischilling/26chan/issues/90).

Migration 0020 adds operator-owned `content.boards.json_tail_size`, default 0
and bounded to 0–500, and `content.threads.undead`, default false. Neither the
public nor staff runtime can change these columns. Zero disables tails. A
thread becomes eligible at twice the configured size in visible replies; the
effective size doubles only when both sticky and undead are true. Deletion can
make a previously available tail disappear. The new flag supplies the original
tail-selection input; it does not implement the unfinished immortal-thread
administration or admission policy.

Operators run `cargo run -p board-store --bin board-migrate --locked` with the
migration login before starting the new release. An existing board keeps tails
disabled until the operator sets its policy, for example:

```sql
UPDATE content.boards SET json_tail_size = 50 WHERE slug = 'example';
```

The old checkout uses a global default of 0, 50 on many boards and 5 on its test
board. The synthetic browser fixture uses 2 to exercise threshold transitions
with a bounded number of actual posts. It is not a production board preset.
Older binaries can ignore the added columns; do not drop them while this
release is running.

`/{board}/thread/{id}-tail.json` works on both public and API listeners. An
eligible response contains the minimal OP state and the latest replies in
ascending order. The OP's `tail_id` is the last omitted reply ID, `tail_size`
is the effective window size, and `replies`/`images` describe the whole visible
thread. The full `.json` response advertises `tail_size` only when eligible.
Missing, deleted, expired or ineligible tails return 404. GET, HEAD, API CORS
and conditional requests retain the read-only API rules. The implemented OP
fields still follow the existing text/development-media subset; unique-poster,
custom-board spoiler and sticky-cap fields remain separate compatibility work.

The public HTML carries the effective initial size in `data-tail-size` on the
thread section. The native renderer uses `/_watch/{board}/thread/{id}/posts`
and `/posts-tail`, both read-only public-listener paths inside the existing CSP
prefix. Projection version 2 includes `tail_size` and nullable string `tail_id`.
An already open version-1 page rejects the new projection until reloaded;
the fixed asset URLs require revalidation, so reload obtains the matching client.
The OP and selected replies retain the shared escaped SSR fragments. Full
counts, board policy, thread flags, boundary and posts come from one read-only
repeatable-read transaction. Image counts include omitted attachments and use
the existing approved-media view, including file deletion and approval removal.

Both projections return content-derived ETags, Last-Modified and
`public, max-age=0, must-revalidate`. The transport sends separate validators
for full and tail responses, along with the original initial date value `0`.
An ETag takes precedence over a date, protecting same-second changes and policy
changes without a new thread timestamp. Date-only comparisons remain
conservative. A 304 is accepted only after a validated representation with a
usable validator; it produces no insertion, event or additional unread count.
Disabling, page exit or failed DOM application clears transport validators.

The client compares the age of its tail reply window with time since its last
completed request. A window that is too old or has an invalid timestamp uses
a full response. A short current document can try a tail. A tail 404 or a valid
response whose boundary is absent from the current thread triggers one full
fallback. A full 404 is terminal. Other failures preserve a retryable page.
IDs stay exact strings; clock calculations avoid signed 32-bit wrapping.

Each update cycle permits at most one tail and one full request, sharing a
ten-second deadline, cancellation signal and 4 MiB streamed-byte budget. The
one-second floor applies between cycles; fallback is immediate within its
existing cycle. Each response uses a disposable parser worker with the existing
two-second, node, text and depth limits. Only a complete validated result can
reach DOM construction. Credentials, redirects, foreign response URLs and
arbitrary stored URLs remain excluded. No CSP source or database grant expands.

Tests cover native thresholds, exact large IDs, independent validators, 304,
missing-boundary/404 fallback, shared byte limits and hung fallback recovery.
Persisted Rust tests cover both listeners, HEAD, counts, policy changes, real
public/staff write denial and concurrent posting. The attachment workflow checks
omitted image counts, file deletion and approval removal. Six browser cases use
actual posts, replies, tail/full responses and API CORS to verify insertion,
draft retention, cancellation, tracked-quote notifications and unread counts.
The CORS client document is synthetic; its loopback permission is scoped to
the test origins, and browser CORS remains active.

Quick Reply coordination, remaining native settings and full rendered-reference
comparisons remain unfinished. These checks do not qualify production media
execution, deployment or whole-application back/forward-cache behavior.
