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
review. The remaining two style-preference scenarios passed after review.
All 57 media scenarios, ten error-state scenarios, three board/catalog scenarios
and six archive scenarios also passed.

Ten captures were reviewed individually. Four serif screenshots remove
post-number underlines to match the pinned desktop CSS. The other six
theme/board/archive captures change a few pixels in the adjacent Reply,
backlink or View thread text. Splitting the number anchors advances those
elements by 1/64 pixel; restoring the single anchor reproduces all six old
screenshots byte for byte. The first hosted Windows run found the board
capture's 15-pixel mismatch; its diff image matched the local reproduction.
The archive review found two further 11-pixel mismatches. Each diff and actual
image was inspected before copying its baseline. Browser pins, zero retries
and zero-pixel tolerances are unchanged.

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
guard. [PR #186](https://github.com/frankischilling/26chan/pull/186) contains this
batch. Its first hosted Windows run stopped at the reviewed board capture;
the next head passed Windows, media/operations, monitoring and dependency
checks. Its Linux application job failed while the inline fixture created a
reply, before that case's browser interaction. The combined gate also failed.
The final head, `3d99eabf423b1ac4b9ef8ade7f5174ad4ae7a960`, passed all six
checks: [Build](https://github.com/frankischilling/26chan/actions/runs/36718197415),
[Monitoring](https://github.com/frankischilling/26chan/actions/runs/36718197099)
and the [Advisory check](https://github.com/frankischilling/26chan/actions/runs/36718198547).
PR #186 merged as `d139e6a6442f5254e5861d888583fedbfb056d9d`. Its merge tree
matches the qualified head. The main-branch
[Build](https://github.com/frankischilling/26chan/actions/runs/36721063411) and
[Monitoring](https://github.com/frankischilling/26chan/actions/runs/36721063485)
runs also passed.

The failed request reused an HTTP socket and stopped immediately about 110
seconds after server startup, consistent with the default retirement window.
Retirement is the likely cause, but the exact race has not been reproduced.
A disposable four-second-budget
probe confirmed that separate Playwright API contexts share a connection;
with `Connection: close`, real fixture posts used fresh sockets before and
after retirement. The inline fixture now sends that header for setup, cleanup
and its API snapshots, and asserts the corresponding response header.
Browser connections, real posting/deletion, request deadlines, server limits
and zero retries remain unchanged. All 36 inline scenarios passed again with
these connection assertions, including the case that failed on the runner.

The preceding staff-posting batch merged in
[PR #185](https://github.com/frankischilling/26chan/pull/185) as
`52d8bc875878bf28cf0512f7e737ed53e520b0f2`, with the same tree as its qualified
head. Its exact-head six checks and post-merge main build/monitoring passed;
see [the staff record](verification-staff-posting.md).
