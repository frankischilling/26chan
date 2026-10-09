# Source board subtitles

`board_subtitle` stores one of `none`, `fiction` or `worksafe_gif`. Migration
0119 assigns `fiction` to `b` and `trash`, `worksafe_gif` to `gif`, and leaves
all other boards at `none`. New boards default to `none`. The migration preserves
existing descriptions and operator state. Descriptions remain separate metadata
and never become banner subtitles.

The template owns both fixed messages. Fiction keeps the source's two lines
and `<br>`. GIF keeps `Worksafe Board: /wsg/` and the `Worksafe GIF` link title;
the destination is local `/wsg/`. Database text cannot supply HTML or a URL.

The deliberate source-compatibility exception is HTML trust: the source accepts
an arbitrary trusted `SUBTITLE` string, while this implementation accepts only
three audited profiles and renders their fixed template content. It has no
`trustedHTML` injection path. The protocol-relative source link is mapped to
local `/wsg/`; arbitrary URLs are rejected.

## Source contract

The [pinned fixture](../tests/fixtures/source-board-subtitles.json) records
source revision `545b7812d1849f7958d914950c91fdbbe38f6b22`, source-file and
excerpt SHA-256 hashes, and byte offsets. Only `b.config.ini`,
`trash.config.ini` and `gif.config.ini` define `SUBTITLE`.

The subtitle follows `.boardTitle` within `.boardBanner` on the index, live
thread, archived thread, catalog and archive index. `imgboard.php`'s archive
list calls the same `head()` function; that function does not exclude
subtitles. `TEXT_ONLY` changes layout classes and catalog controls without
changing subtitle emission. The forced `0ch.css` stylesheet's 10pt rule does
not apply to every text-only board.

The fixture also records the six desktop/catalog theme rules and the two
mobile rules. Browser checks reuse the existing hash-keyed
[public component measurements](public-page-chrome-reference.json) for
computed subtitle styles. They check title/subtitle placement, line breaks,
responsive wrapping and horizontal overflow at 390, 480, 481 and 1280 pixels,
with display densities 1 and 2.

## Checks

Run the source checks without a database:

```sh
node --test tests/browser/source-board-subtitles.test.mjs
SUBTITLE_REFERENCE_DIR=/path/to/reference node --test tests/browser/source-board-subtitles.test.mjs
```

The second command also compares the recorded bytes with the reference files.
To deliberately re-record from that revision:

```sh
node tests/fixtures/source-board-subtitles-record.mjs /path/to/reference
```

With a migrated, isolated development database and the real public server
running, the HTTP runner needs no browser:

```sh
APP_ENV=development SUBTITLE_HTTP_ORIGIN=http://127.0.0.1:3000 \
  node tests/browser/source-board-subtitles-http.mjs
```

Set `MIGRATION_DATABASE_URL` through the development environment. The fixture
accepts only the `board_migrator` role on `127.0.0.1`, rejects visual fixtures,
and never prints database diagnostics. It reads the real `b`, `trash`, `gif`
and `g` pages, then commits random owned boards with live and archived OPs to
exercise all five page modes. Cleanup requires exact ownership markers and
runs after a failed assertion or uncertain seed acknowledgement.

Browser checks start the real public application through the existing public
Playwright configuration:

```sh
npx playwright test --config playwright.source-board-subtitles.config.js
```

These checks use persisted HTTP responses, including reloads, disabled
JavaScript, keyboard navigation to `/wsg/`, hostile board metadata, both
safety modes and text-only layout. They do not intercept the application
responses or inject subtitle markup. Chromium must already be installed.
Source and HTTP checks do not establish browser geometry; report the browser
run separately if Chromium is unavailable.

Migration preservation and runtime authority are covered by
`scripts/test-board-subtitles-migration.sh`,
`apps/public/tests/board_subtitles.rs`, and the source importer tests.

## Validation status

The two source-import Python checks and six Node checks pass, including full
source-file hash verification. Real-role subtitle HTTP tests, board inventory
tests and 129 database-feature public library tests also pass. One Unix-socket
transport test was explicitly filtered because this environment prohibits that
transport.
Full-workspace validation was not run; its dependency set is not cached.
The standalone HTTP runner passes all 28 real persisted-page checks, including
`b`, `trash`, `gif`, `g` and every owned mode; fixture seeding and cleanup pass.
Playwright discovers all 15 subtitle tests. Browser geometry and browser
interaction remain unqualified here because Chromium is unavailable; no
browser was installed.

Hosted Windows screenshots from head `4188db1` exposed the expected removal of
the generic board description. Fourteen baselines were reviewed against their
expected, actual and diff images: the two catalog-limit views and twelve
no-JavaScript theme views. Their dimensions, content and horizontal layout are
unchanged. Content below the omitted description moves up 11px, or 16px in the
desktop Futaba and Burichan themes. The replacement PNGs are those captured
images, not locally reconstructed pages.

This review does not qualify the other screenshot suites or fix the separate
Windows socket failure while loading the watcher script. Complete hosted
checks remain required before merge.
