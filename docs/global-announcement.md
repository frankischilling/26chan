# Global announcement

A single operator-published message appears above the posts on board indexes,
catalogs and open threads. It is separate from the board blotter. Ordinary
closed and archived threads omit it. The source upload-board template also
shows the message on closed threads; that exception is retained. The body is bounded plain text and stays escaped;
original arbitrary announcement HTML is not imported.

## Publishing and withdrawing

Apply migration 0157 before this binary and retain it during binary rollback.
An older binary ignores the announcement without erasing its record. Use the existing local-content editor
with the actual migration login:

```json
{"kind":"page","version":1,"key":"announcement","expected_revision":0,"title":"Site announcement","body":"An operator-written announcement."}
```

```sh
board-site-content publish announcement.json
board-site-content page-revision announcement
board-site-content retract-page announcement 1
```

`MIGRATION_DATABASE_URL` must name the `board_migrator` login. The title has a
256-byte bound and supplies an accessible label; the body has an 8,192-byte
bound. Updates use the current revision. An outdated editor cannot overwrite
newer content. Withdrawing the page removes the announcement on subsequent
requests. Runtime roles cannot publish it or read the private editing table.

The existing page record supplies its last-update timestamp, rounded to Unix
seconds for the public `data-utc` attribute. Desktop dismissal uses
`4chan-global-msg`. A strictly newer timestamp is shown on the next navigation;
an edit within the same second can retain the previous dismissal, as with the
source file's modification time. Expanding the announcement removes that
preference. The key is not part of Settings export.

## Source behavior

The reference is revision `545b7812d1849f7958d914950c91fdbbe38f6b22`:

- `lib/ads.php:134–143` reads the global message and its file modification time.
- `views/imgboard.php:263–267` supplies its visibility gate and DOM fields.
- `js/extension.js:9753–9801` supplies desktop dismissal and timestamp comparison.
- `js/catalog.js:408–460` supplies the independent catalog control.
- `js/core.js:1546–1567,2452–2455` supplies the mobile control.

The mobile announcement starts collapsed. Its core toggle works even when the
native extension is disabled, changes between the source open/close captions,
and leaves desktop dismissal unchanged. Desktop board controls follow the
extension's enabled state. Catalog dismissal is independent of `disableAll`
and requires available WebStorage. The timestamp comparison preserves the
source's string comparison, with bounded inputs.

Storage failure leaves the board toggle usable for that page. Accessible
buttons replace the source's image/span click targets; theme-owned plus/minus
images retain the visual affordance. Controllers own their listeners and style
changes, suspend across BFCache, and stop acting after disposal or replacement.
They do not load external content or process announcement text as HTML.

The mobile button retains the source base color, border and geometry. The
original `buttonfade-red.png` is absent from the supplied files; retrieval from
its source URL returned a 195-byte HTML “Site Unavailable” page on October 15,
2026, not an image. The button currently uses the flat source color. This
remaining artwork difference belongs to page/reference work and is not claimed
as pixel-identical. Desktop message colors follow the source theme families.

## Checks

`record-global-announcement.mjs` executes only the pinned announcement functions
against a synthetic DOM and storage. Its retained vectors cover board/catalog
initial state and two toggles for missing, empty, older, equal and newer stored
values, including the source's lexical ordering. Separate vectors capture both
mobile captions. Changes to the pinned snippets fail verification.

The source and timestamp unit checks pass locally. Actual store-module tests
pass against PostgreSQL with migration 0157, including publication, stale
revision rejection, withdrawal and runtime read-only grants. The populated
upgrade test passes drift rejection, rollback, retained page/news fingerprints,
public projection and administrator dump/restore.

HTTP tests cover escaped index/catalog rendering and withdrawal. Browser tests
compare the real controller to the source vectors, mobile behavior, disabled
extension/catalog differences, denied storage, settings changes, BFCache and
cleanup. These HTTP/browser cases still require exact-head hosted execution;
compiled or source-only checks do not establish that they passed.
