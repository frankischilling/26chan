# Catalog spoiler reveal

Issue: https://github.com/frankischilling/26chan/issues/86

The pinned public catalog v1025 client stores `nospoiler` in `catalog-theme`,
toggles `reveal-img-spoilers`, and switches available spoiler cards between the
spoiler asset and normal thumbnail sizing. Source provenance is recorded in
[the catalog reference](public-catalog-reference.json). No upstream code runs
in the application or the verification browser.

The local toolbar offers Hidden and Reveal. Only the boolean `true` restores
reveal from at most 4096 code units of optional browser storage. Other settings,
including arbitrary CSS, are never applied. Saving this supported option writes
only its finite object; hiding or resetting removes that object. Unavailable
storage leaves the current control usable.

The finite `spoilers=on/off` GET parameter provides no-JavaScript operation and
takes precedence over browser restoration. This toolbar and URL extension do
not reproduce the entire native options panel. Board-specific spoiler artwork
remains a separate gap.

Hidden is the default. Spoiler metadata contains an escaped, server-generated
approved-media URL, not an active image source. Client rendering changes only
cards selected by the current search/hidden view, so restoring or toggling reveal
does not activate initially inert or locally hidden cards. An explicit server
GET with `spoilers=on` already requests revealed rendering; client-local hidden
state cannot retroactively prevent its initial server-rendered requests.

Revealed images use the regular small/large bounds and retain thread links, pins
and menus. Switching off restores the fixed spoiler asset and its 100 by 100
dimensions. Deleted files expose no reveal source and cannot be revived by a
preference. Legacy approved attachments without thumbnail metadata use the
existing bounded normalized-image fallback, never the original upload.

Coverage includes finite option validation, persisted HTTP spoiler/deletion
cases, the real no-JavaScript upload/reader workflow, six-theme desktop/mobile
properties, optional storage and request witnesses for initially hidden and
filtered cards with successful revealed-image controls. Exact results belong
on the PR revision; these descriptions alone are not qualification evidence.

Production media remains disabled and unqualified.
