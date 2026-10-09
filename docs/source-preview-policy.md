# Source board preview policy

The supplied source uses REPLIES_SHOWN for HTML board pages and JSON index and
catalog previews. Its default is five replies. The b and bant boards use three,
t uses one, and vg uses zero. Sticky threads show at most one reply, without
increasing a configured zero.

The board policy is stored as replies_shown. Source-facing reads select each
thread's visible replies and preflight their saved-comment budgets inside the
same repeatable-read transaction. Explicit fixed-limit store helpers remain
available for internal callers; they do not define the public board policy.

Public previews also require a live opening post before pagination. This avoids
rendering orphan replies as a thread when its opening post is unavailable.
That visibility check is backend hardening. Metadata-only and internal staff
projections retain their existing behavior.

## JSON fields

Index and catalog previews report omitted_posts and omitted_images when replies
are omitted. An omitted image count of zero remains an explicit value in that
case. Catalog last_replies is absent when the thread has no replies, but is an
empty array when replies exist and the configured preview limit is zero.

Preview entries omit unique_ips. Full and tail thread responses retain their
existing source-controlled unique-poster fields. Quote destinations, capcode
reply groups and private-field exclusions keep their separate contracts.

## Native page updates

The internal native board-page response uses version 2 and includes the board's
replies_shown value. The browser checks the exact selected-post count for each
thread against its reply count, sticky state and board policy. It does not infer
that policy from a board name or accept an arbitrary number of posts.

A page can contain at most 20 previews, each with one opening post and at most
five replies. The page admission bound is therefore 120 posts. Existing byte,
node and whole-depager limits remain in force. Version 1 responses are rejected;
the ordinary board link remains the fallback for an unavailable incremental
page. Public assets require cache revalidation.

## Qualification

Local validation passes: 143 domain tests, 34 store unit tests, five store
database tests, 14 public HTTP/snapshot tests and 137 public library tests.
One Unix-socket transport test is explicitly excluded because this environment
cannot create Unix sockets. All-target checks and Clippy with warnings denied
pass for board-public and board-store.

The source fixture contains 172 controlled-cache cases consumed by domain and
public JSON tests. Its 12 Python tests pass, as do 86 focused client Node tests
and the generated-asset checks. The fixture models deleted rows as prefiltered
input; it does not establish a historical database visibility policy. No
original PHP application is executed.

Browser execution remains for hosted CI because Chromium is unavailable
locally. This policy does not establish full page-layout, media or rewrite
parity.
