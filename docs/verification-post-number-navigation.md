# Post-number and header verification

The change separates post permalinks from reply controls, adds a
script-free quote form and records public desktop staff-header facts. It
advances B-015 and the local reference work in #6. It does not close the
remaining catalog, original-server, mobile-header or production requirements.

## Local results

The restricted-database HTTP regression passed. It checks board and thread
HTML, updater and one-post projections, real prefilled forms, canonical
numbers, duplicate parameters, foreign and deleted posts, closed and archived
threads, unchanged public JSON and read-only GET behavior. The archive fixture
uses an enabled board with paired archive timestamps and a future expiry.
The locked, offline, all-feature Rust workspace run passed 543 tests in 154
result groups, with no failures or ignored tests. Its preview regression checks
that archiving changes the reply-number target and invalidates the old ETag
while preserving the rest of the fragment. Workspace all-target/all-feature
Clippy passed with warnings denied.

All ten persisted Quick Reply browser scenarios passed. The new cases check
normal permalink navigation, quoting an original and a live-inserted post,
real script-free posting at desktop and mobile widths, and the visible mobile
ordinary form when Quick Reply is disabled. Existing draft preservation,
selected text, Ctrl-click, posting CSP, Unicode advice and updater coordination
also passed.

The source reproduction passed all 72 desktop cases on Linux and Windows with
no external request. The production-template checks cover six styles with
twelve OP/reply badge cases each, two mobile density checks, and six existing
desktop layout scenarios. The expanded normal-link and hover comparisons
passed all eight header scenarios on Windows. The reference invocation follows
the released Depager's explicit OP flag. The reviewed owned mobile capture fits
all twelve headers and loads the fixed icons; original mobile layout is still
unqualified.

All 389 JavaScript tests passed with two test files running concurrently and
unchanged request deadlines. This includes 41 parser/inline-DOM cases for forged
controls, exact large IDs, local/remote copies, worker transport, cancellation
and aggregate bounds. The near-limit inline fixture now accounts for three extra
nodes in the split header links. The 16,384-node production limit is unchanged.
The persisted preview/backlink browser suite passed all 82 scenarios; the
inline-quote browser suite passed all 36. Windows Quick Reply, ordinary-form
viewport transitions and closed-thread controls passed all 29 scenarios after
the mobile prefill reveal was restricted to an actual quoted form.

The final filter/parser release asset is 248,832 bytes within its 262,144-byte
limit. Quick Reply is 24,377 bytes within its 32,768-byte limit. Their source
guards retain fixed imports, approved source files and pinned dependencies.

The broad Windows theme suite passed 156 of 158 scenarios before baseline
review. The remaining two style-preference scenarios passed with the seven
reviewed captures: four serif screenshots remove post-number underlines to
match the pinned desktop CSS, and three desktop sans-serif screenshots change
a few pixels in the adjacent Reply link. Splitting the number anchors advances
that link by 1/64 pixel; restoring the single anchor reproduces the three old
screenshots byte for byte. All seven diff images and captures were reviewed
before copying those baselines. Browser pins, zero retries and zero-pixel
tolerances are unchanged.

```sh
cargo test -p board-public --all-features --test post_number_links --locked --offline
cargo clippy -p board-public --all-targets --all-features --locked --offline -- -D warnings
node scripts/verify-public-header-reference.mjs /path/to/extension.1191.js /path/to/desktop-css
node --test tests/browser/native-updater.test.mjs tests/browser/native-inline-quotes-dom.test.mjs
npx playwright test tests/browser/quick-reply.spec.js
npx playwright test --config playwright.themes.config.js tests/themes/post-headers.spec.js tests/themes/post-layout.spec.js
```

## Remaining qualification and hosted state

The final persisted mobile-prefill regression passed after narrowing the reveal
guard. Hosted qualification and the matching-head merge have not yet been
recorded for this branch. Local results above do not substitute for those checks.

The preceding staff-posting batch merged in
[PR #185](https://github.com/frankischilling/26chan/pull/185) as
`52d8bc875878bf28cf0512f7e737ed53e520b0f2`, with the same tree as its qualified
head. Its exact-head six checks and post-merge main build/monitoring passed;
see [the staff record](verification-staff-posting.md).
