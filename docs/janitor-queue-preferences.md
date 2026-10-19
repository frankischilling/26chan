# Queue preferences and template search

The request queue Settings panel remaps the supported keyboard actions and
filters the board menu. Preferences stay in browser storage. The queue does not
store posts, identities, CSRF values or review payloads there.

The source key alphabet is A–Z, Left, Right and Escape. All nine implemented
queue actions can be remapped. A save validates the complete mapping before
changing any key; duplicate assignments are rejected. Invalid or oversized
saved mappings fall back to defaults. Input fields, buttons and modified or
repeated key events retain their existing protections. Escape still dismisses
an amendment while its fields are being edited.

Remaps use the source `br-keyremap` numeric pairs. Board lists use
`rq-settings.customMenu`. Saving a menu preserves other settings in a usable,
bounded record; an oversized existing record is left intact. If storage cannot
be written, preferences still apply to the current page. Reset restores native
keys and the full board list without deleting unrelated source settings.

A custom menu is a comma-separated list of exact board slugs. Matching retains
the source's handling of whitespace and empty comma fields. More boards
reveals the full authorized menu temporarily. Refresh reapplies the saved list.
This filters navigation only; it does not grant access to another board or hide
requests in the currently loaded queue.

## Amendment search

Find template selects the first available template whose name matches every
space-separated word at a word boundary, ignoring case. It uses the source
word-prefix behavior, including the initial empty-word stopping condition.
Disabled templates are skipped. Queries are limited to 256 characters and 16
words. Regex control characters are treated literally to avoid executing a
user-provided expression on the staff page.

Changing the selected template updates whole-day duration and scope. Indefinite
uses -1 and a warning duration uses 0. A fractional or out-of-range duration
leaves the explicit length blank so the server retains the template's duration.
Changing templates restores the selected public reason; private notes remain.
The reason is displayed as escaped text. See [request history](janitor-request-history.md).

Without JavaScript, the ordinary template selector and explicit amendment
fields remain available. Search and browser preference controls stay hidden.

## Verification

`fixtures/janitor-preference-source.json` pins isolated source fragments for the
key alphabet, custom menu and amendment search. The state tests execute those
fragments on synthetic fields and compare their decisions with the native
client. Other tests cover malformed storage, duplicate bindings, reset, failed
storage writes, unavailable templates and literal regex characters.

```sh
node scripts/check-janitor-preference-source.mjs /path/to/4chan-old
node --test tests/browser/janitor-queue-state.test.mjs
RUN_JANITOR_BROWSER=1 cargo test -p board-staff --features database-tests --test janitor_requests --locked
```

The browser fixture covers saved preferences after reload, remapped warning
Undo, template search and duration changes on desktop and mobile. Browser and
full CI qualification are required before merge. These controls do not establish
full source visual parity or complete issue #223. Retained media, thumbnail
controls, private staff-history tools and source-pixel comparison remain open.
