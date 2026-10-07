# Source spoiler thumbnails

Server posts, catalogs and native post copies use the supplied source's separate
spoiler rules. [The metadata reference](custom-spoiler-metadata.md) records all
82 board configurations. The server chooses one `SPOILER_THUMB` image for a
rendered page or updater snapshot and shares it across its posts. Catalogs use
`spoiler-{board}{SPOILER_NUM}.png` when the enabled count is positive, otherwise
the generic image.

The native client caches one choice per board in each document. On the current
board it reuses the first rendered custom spoiler image. For another board,
or a current board without that image, it chooses an integer from one through
the advertised count. Later count changes and zero do not replace a cached
choice. The reveal preference suppresses the first cache choice. Previews,
inline quotes, updates, expansion and added board pages share this cache across
their browser bundles. Navigation creates a new document and cache.

The source has three HTML aliases that differ from the catalog/native suffix:

| Board | Server HTML | Catalog and cross-board native copy |
| --- | --- | --- |
| `news` | `spoiler-a1.png` | `spoiler-news1.png` |
| `vm` | `spoiler-v1.png` | `spoiler-vm1.png` |
| `vst` | `spoiler-vst.png` | `spoiler-vst1.png` |

Reveal and conceal retain the selected placeholder. Deleted files keep their
deleted-file image. No-file cards keep their no-file image. UI placeholders
stay at 100 by 100 CSS pixels at both tested display densities.

## Policy and assets

Apply [migration 0082](../migrations/0082_spoiler_thumbnail_assets.sql) before
starting the updated binaries. It imports the independent HTML thumbnail
arrays, with the generic image as the default for operator-created boards.
It preserves spoiler enablement, counts, saved posts, attachments and grants.
Only migration authority can change this policy. Arrays contain one through
64 fixed release names, with one dimension, a starting index of one and no
null elements. Arbitrary URLs and paths are rejected.
Public readiness requires both spoiler policy columns.

[The asset record](custom-spoiler-assets.json) lists 45 names used by the source
rules, including 43 PNGs retrieved from the public CDN on October 3, 2026.
Their hashes, byte lengths, dimensions, URLs and retrieval times are recorded.
These are retrieved UI bytes, not files bundled in the supplied checkout or
proof of historical pixel identity. The release serves each available image
from a fixed route and permits those routes in its image CSP. The application
does not fetch these files from the CDN at runtime.

`spoiler-news1.png` and `spoiler-vm1.png` were unavailable at collection. Their
catalog and cross-board native paths return 404. Their server HTML aliases
remain available. They have no fabricated substitute. Counts or board names
without recorded release assets cannot grant new image URL authority to a
native recipe; the affected native operation falls back to ordinary navigation.
Adding new artwork requires a release update and its provenance record.

## Verification

[The PHP extractor](../scripts/extract-custom-spoiler-reference.py) evaluates
the pinned INI policies and regenerates migration 0082. [The client extractor](../scripts/extract-custom-spoiler-client-reference.py)
records the actual source cache function and catalog expression. Node executes
those expressions with controlled randomness and image lookup, then compares
the new client across every positive-count source board, every HTML choice,
cache reuse, zero/count changes and separate documents.

[The populated migration check](../scripts/test-spoiler-thumbnail-migration.sh)
compares historical records, privacy policies, functions and grants before
and after the upgrade. It checks aliases, defaults, array constraints and
seven actual runtime write denials. [Persisted public checks](../apps/public/tests/custom_spoilers.rs)
compare all 82 imported arrays and verify the real asset routes, hashes,
CSP entries and the two 404 responses. Public-library checks exercise every
source HTML choice through the post template and a shared snapshot choice.

[Client regressions](../tests/browser/custom-spoilers.test.mjs) enforce the
complete file grammar and canonical, root-only count metadata. [Browser contracts](../tests/custom-spoilers/custom-spoilers.spec.js)
use production templates with synthetic approved-image fixtures on desktop
and mobile at density two. They cover the aliases, random source choices,
catalog reveal/conceal, native preview caching, deleted files and missing UI
assets. These fixtures do not qualify media decoding or isolation. Existing
media and security checks remain separate.

```sh
python3 scripts/extract-custom-spoiler-reference.py 4chan-old \
  apps/public/tests/fixtures/custom-spoilers.json --check \
  --migration migrations/0081_custom_spoiler_counts.sql \
  --thumbnail-migration migrations/0082_spoiler_thumbnail_assets.sql
python3 scripts/extract-custom-spoiler-client-reference.py 4chan-old \
  apps/public/tests/fixtures/custom-spoiler-clients.json --check
sudo bash scripts/test-spoiler-thumbnail-migration.sh
cargo test --locked -p board-public --all-features --lib --test custom_spoilers
npm run test:custom-spoilers
```

Issue #217 remains open for the two unavailable spoiler images, unavailable test
flag artwork and complete historical visual qualification. [The flag inventory](source-board-flags.md)
records the 165 source definitions and available rendering evidence.

## Browser evaluation ownership

At `66fcdfd`, the Linux custom-spoiler suite passed 11 cases and failed the mobile
DPR-2 preview case with `Resulting promise was garbage collected`. The retained
trace shows the same page/frame and rendered board before and after the failed
evaluation. All 175 recorded network entries returned HTTP 200, both imported
modules had loaded, and no navigation or runtime exception was recorded during
the roughly 22-millisecond call.

Playwright maps Chromium's `Promise was collected` response to that message.
The artifact does not identify the underlying collection cause or establish a
spoiler-rendering defect. The fixture now keeps each async result in an ordinary
object owned by a JSHandle until the result is read, then disposes that handle.
Its module imports, preview construction, reveal/conceal steps and assertions
are unchanged. Manager and temporary DOM cleanup run in `finally` blocks.

A separate control keeps a result pending behind a release barrier while
requesting browser garbage collection, then releases it and checks the result.
This verifies the explicit ownership path without adding retries, browser flags
or longer timeouts. Syntax and discovery pass locally with 13 cases; actual
browser execution requires the next hosted run. This is fixture ownership
hardening, not a proven explanation of the original Chromium failure.
