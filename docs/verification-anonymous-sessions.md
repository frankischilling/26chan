# Anonymous-session verification

This records the automatic posting/deletion slice under #198 in the existing
rewrite-completion PR. It does not qualify the whole rewrite. The supplied
source and replacement policy are described in
[anonymous-sessions.md](anonymous-sessions.md).

## Reference and private state

The hash-pinned extractor matches 1,370 known-user decisions, 966 activity
transitions and 12 idle resets from PHP 8.3.6. It executes only the 23 named
pure methods on synthetic state. It excludes the original constructor,
cookie cryptography and runtime configuration.

```sh
php scripts/extract-anonymous-reference.php /path/to/4chan-old fixtures/anonymous-reference.json --check
cargo test --locked -p board-domain --test anonymous_session
cargo test --locked -p board-store --all-features --test anonymous_session
```

The ten domain tests pass. They compare every source vector, check capability
encoding/randomness and fingerprint separation, and exercise generated
activity sequences. The nine store tests pass against PostgreSQL 16.15 with
the actual restricted public, intake and media logins. These checks cover:

- All 486 non-dummy source transitions through the persisted activity helper.
- Successful posts/reports, idle reset and address/network/country changes.
- Missing, revoked and expired authority with no partial content or reports.
- Membership removal, session revocation, password rotation and expiry after
  a deletion request demonstrably reaches a held board lock.
- Anonymous OP formatting after that same lock wait, with source address and
  legacy-password paths excluded from the fixture. Removed membership or a
  rotated hash produces an ordinary reply; a revoked/expired session commits
  no reply. A valid current membership supplies the positive control.
- Two same-session replies on different boards, both reaching a held session
  lock before release. Both complete with their current OP formatting rights;
  the final-strength lock avoids a shared-lock upgrade cycle.
- A real approved attachment remaining reusable after failed registration,
  its one-use consumption and canonical image activity on success, and
  cookie-authorized file-only deletion preserving the post.
- Public-login denial of private reads/writes, arbitrary activity updates,
  owner-role assumption and schema creation.

The first attachment fixture had its board's image limit at the default zero.
It correctly failed before reaching anonymous registration. Enabling images
on that owned board makes the intended transaction test pass. No production
limit or assertion was removed.

## HTTP and browser behavior

```sh
cargo test --locked -p board-public --all-features --test anonymous_session
cargo test --locked -p board-public --all-features --test http_limits
node --test tests/browser/native-updater.test.mjs tests/browser/native-quick-reply.test.mjs tests/browser/native-post-preferences.test.mjs
npx playwright test tests/browser/anonymous-session.spec.js
```

The three HTTP tests pass: automatic URL-encoded and multipart posting,
multi-request ownership, exact source JSON envelopes, private-cookie flags,
seven-day preferences, cacheable reads with no private state, reports,
malformed/duplicate cookies and outsider deletion denial. The nine admission
tests also pass. Their large percent-encoded form now includes an invalid
thread ID to reach form validation without a database; an omitted password
is valid for the source form and no longer supplies that rejection.

All 43 targeted JavaScript tests pass. All six new browser cases pass on
desktop/mobile, with JavaScript enabled and disabled. They cover ordinary
forms, Quick Reply, multiple tabs, HttpOnly cookie handling, storage rejection,
cookie reset and forged display receipts. Storage rejection is distinct from
actual cookie rejection; the existing full-Chromium content-setting test
checks that separately.

The first browser attempts stopped on fixture setup: an unavailable database
environment, filling a collapsed posting form, and a missing QR field ID.
The corrected fixture opens the source toggle and the hidden QR input now
retains `qr-pwd`. A concurrent Cargo build then prevented server startup;
subsequent browser runs are serialized after compilation.

The next run exposed an application regression: Quick Reply committed its
post, but the finite updater parser rejected the new hidden deletion input.
The parser now accepts the empty field only for its own post's delete action.
Regression cases reject a supplied value, foreign post ID, unrelated hidden
credential, visible text field or report action. The filter and global-search
bundles were regenerated from pinned sources. The six browser cases then pass.
Failed logs and traces remain available in the local verification artifacts.

The broader affected browser batch ran 146 cases: 143 passed and three failed
in post-tracking cleanup. Those posts had been created by the browser's
automatic capability, while teardown still supplied a fixed legacy password
through an independent request context. Cleanup now retains the private cookie
issued for each owned post, including the no-JavaScript context before it
closes. All four post-tracking cases pass in the focused repeat. Together,
these runs supply passing evidence for all 146 affected cases; the original
batch remains recorded as three failures. The real cookie-rejection case also
passed in that batch. The original failure traces are retained separately.

All 36 cases pass in the separate full-Chromium inline-quote configuration,
including the original-form preservation and copied-credential exclusion case.

The focused visual-fixture runs pass 23 theme cases and 27 Quick Reply/media
menu cases. All six themes retain desktop/mobile field geometry, collapsed
and expanded form controls, quote editing, hidden-password validation and
menu selection without submitting. The diagnostics checks still exclude form,
cookie, storage and URL-query contents. The local image review covered expanded
Yotsuba desktop/mobile forms, Tomorrow and Futaba mobile forms, Yotsuba and
Photon mobile Quick Reply, and Tomorrow desktop Quick Reply. The hidden
credential adds no visible row, and the reviewed forms fit their viewports.
These are focused checks; complete original-page comparison remains #193.

All six generated client bundles match their pinned sources and existing
budgets. The complete set of 50 browser-client test files and two client
JavaScript test files passes 477 tests, with no failures, skips or cancellations.
Formatting and workspace Clippy with warnings denied pass. The complete Rust
workspace repeat passes all 652 tests with no failures or ignored tests across
169 completed test targets, including empty documentation-test targets. This
run uses the verified owned PostgreSQL 16.15 cluster with all 56 migrations,
including the final-strength anonymous lock in migration 0065.

## Migration and recovery

```sh
sudo bash scripts/test-anonymous-session-migration.sh
```

The complete private-cluster exercise passes. It applies migrations through
0064, seeds historical content and deletion hashes, applies 0065, and proves
those historical rows are unchanged and receive no invented membership. It
checks the SET-only migration membership and every runtime login's denied
private authority. Capacity exhaustion and a missing policy roll back the
entire registration transaction. Garbage collection removes at most 64 of
65 expired synthetic sessions.

A populated dump/restore preserves all four new private tables and returns
the same usable post-deletion proof through the restored public login. That
login still cannot read or update the private tables. The first denial fixture
used `SET ROLE` from a superuser session, which could assume the owner through
its original session identity. The corrected check connects as the actual
public login. The application grants were unchanged.

An existing local database rejected migration 0064 because it contained an
older, unpublished migration checksum. It was preserved. Qualification uses
a fresh owned database with all 56 migration records, rather than rewriting
the stored checksum. The first broad workspace attempt lacked Node on its
helper PATH; the second reached the obsolete required-password admission
fixture described above. Both failed runs remain incomplete evidence.
The next repeat found one remaining old password fill in the board-flag QR
fixture. It now asserts the source's empty hidden field. The same adjustment
was made to the approved-image reply's file-only deletion fixture.
That flag helper initially used an assertion function it had not imported.
Its existing Node assertion now checks the empty hidden field, and the focused
flag/browser integration test passes. Delete-menu selection now focuses the
submit button when the credential field is hidden; selection still sends no
request. Draft-preservation cases use the comment/report fields. The inline
copy case keeps a synthetic private value in the original hidden control and
checks that copying a quote does not duplicate it or alter the original form.
The approved-upload workflow then found an obsolete selector and one-year
display-cookie expectations. Its checks now use `postPassword`, distinguish
the year-long HttpOnly anonymous cookie from seven-day display preferences,
and retain exact receipt and preference assertions. The focused upload browser
test passes for all six workflows: ordinary and attachment-only posts with
JavaScript enabled and disabled, plus confirmation-page and inline Quick Reply.
These checks include one-use approvals, replay denial and file-only deletion.

The next full workspace attempt passed 640 Rust tests before the aggregate
monitoring fixture rejected a missing `BOARD_TEST_CLUSTER` marker. The local
helper now checks the generated cluster name against PostgreSQL's actual
`data_directory` and verifies all eight database URLs and role names before
exporting that marker. The focused monitoring check passes with its existing
empty-queue, unsafe-grant and restoration assertions. The fixture guard and
application code were unchanged; the failed broad attempt remains recorded.

## Remaining scope

The anonymous state supplies private distinctions for admission hooks,
CAPTCHA and Pass. Those consumers remain unfinished under #212, #199 and #201.
Complete deletion policy remains #214. Source cookie import, configured
third-party classifiers and operator private data are not claimed.

## Native Windows screenshot recovery

On `1d928b9`, [run 36991846844](https://github.com/frankischilling/26chan/actions/runs/36991846844)
passed media and operations qualification, while the Windows job failed two
archive screenshots. Both expected images still contained the visible deletion
password control. The main visual cases and the other four archive cases passed.
The failure images were retained and compared with the checked-in baselines;
their expected-image hashes matched before any baseline was changed.

The native repeat uses Rust 1.94.0, Node 24.14.0 and Playwright 1.62.0 with the
existing pinned Chromium, viewports, fonts, fixtures and zero-pixel tolerance.
All six archive cases pass after reviewing and updating only the two opened
deletion-form images. The posting-form checks then found twelve theme images
and four media board/thread images containing the obsolete visible password
row and password-saving help text. Every diff was reviewed. Removing that row
also narrows the desktop label column and moves following content upward.
The ten catalog/read-only media cases already passed without baseline changes.

Only those eighteen reviewed PNGs were updated. Assertions now check that the
source password controls are empty and hidden while retaining the posting and
deletion field names. The repeat passes both theme cases, which check all six
persisted styles on desktop and mobile, and all fourteen media screenshot
cases. Together with the archive repeat, all twenty-two targeted native cases
pass. The first theme run remains recorded as two failures and the first media
run as four failures plus ten passes; their images, traces and logs are retained.
No application code, fixture content, screenshot tolerance, deadline or retry
setting changed during this recovery. This targeted result does not replace
complete current-head CI or the original-page comparison in #193.

The earlier integrated checkpoint `3b6300a` passed all required jobs in
[run 36966578661](https://github.com/frankischilling/26chan/actions/runs/36966578661).
This newer slice still requires its own complete current-head CI. PR #227
remains a draft until the full rewrite scope and final checks are complete.
