# Board math display

Migration 0113 imports the effective JSMATH policy from all 82 supplied board
configurations. It is disabled by default and enabled on `/sci/`. Enabled board
and thread pages load a fixed local controller through watcher bootstrap. Catalogs, disabled boards,
error responses and JSON responses receive no math-script or worker permission.
`boards.json` includes `math_tags: 1` only for enabled boards.

The supplied PHP call to `jsmath_parse` is commented out. Stored comments and
API responses therefore keep literal `[math]...[/math]` and `[eqn]...[/eqn]`
tags. Rendering is a browser projection. Inline math and left-aligned block
math use those explicit, case-sensitive delimiters; ordinary dollar signs do
not activate it. Initial page rendering is independent of extension settings. The module does
not mount on import. Watcher first establishes preferences and publishes
`4chanMainInit`, then explicitly mounts math before the other post features.
A delayed import waits while the page is suspended and resumes once on persisted
restoration; a terminal departure cancels mounting.
Quick Reply has a separate TeX preview input with a 50 ms debounce. Its input
does not alter the posting draft.

## Reference and renderer

`tests/fixtures/native-math-source.json` contains independently extracted
source snippets and SHA-256 hashes for detection, policy, delimiter settings,
dynamic parsing and the separate preview. Regenerate or check it with
`scripts/extract-math-reference.py` and the supplied original checkout. The
oracle executes the original detector and loader configuration with inert
stubs. It does not compare equation rendering.

The original page loads the remote `2.6-latest` MathJax distribution, whose
exact bytes are absent from the supplied checkout. This implementation uses
the pinned `@mathjax/src` and `@mathjax/mathjax-tex-font` 4.1.3 packages. It does
not claim identical historical pixels or support for every TeX extension.
Both packages are build inputs; pages contact no font service or CDN.

`scripts/build-native-math.mjs` bundles the direct TeX/SVG implementation and
TeX glyphs from an explicit input allowlist. It checks package versions,
imports, exports and release sizes. The worker includes the full Apache code
license and the glyph copyright/OFL notice from `licenses/mathjax-tex-font.txt`.

## Resource and DOM boundaries

The controller starts one worker on demand. Jobs have a 1.5-second deadline,
a queue ceiling of 256 and at most three replacement workers after failures.
Source length is limited to 4,096 characters per equation. Parser calls,
nesting, column-template expansion, alignment counts, intermediate math nodes
and output nodes/bytes are bounded before the relevant allocations or recursive
passes. Every job receives fresh parser and document state.

The local renderer excludes extension loaders, user definitions, HTML, URLs,
styles and dynamic font loading. The worker has a networkless response CSP.
The main thread independently validates returned geometry before creating SVG
elements. Only bounded `svg`, `g`, `path`, `rect` and `line` geometry is accepted.
Nested viewports retain their clipping behavior; the outer SVG is clipped by
a fixed attribute and external stylesheet. No worker-provided markup is parsed
as HTML.

Cache limits are 64 entries and 2 MiB, including keys. Live output has separate
limits of 256 equations, 32,768 nodes and 4 MiB. Observer targets and retained
messages are capped at 512, and each scan is bounded. Excess or rejected work
stays literal rather than expanding a budget.

Original text, HTML and clone readers share private ownership records for
generated output. Completion checks the full source projection and node order,
so stale results cannot replace edited content. Removal, preview closure and
page suspension release output and pending work. A restored page can typeset
again. This source projection does not install a custom clipboard handler or
claim that native text selection copies TeX identically to the old renderer.

## Qualification and rollout

Apply migration 0113 before starting binaries that read `math_tags`. Existing
comments are not rewritten. A binary rollback retains the additive column and
saved comments, but an older client does not provide the new display feature.

Local checks cover source extraction, the actual math engine, bounded geometry,
literal source projections, real database policy/API behavior, and production
security headers. Browser cases use a read-only fixture server with production
templates and CSP, including real worker rendering, dynamic quotes/updates,
preview cancellation and hostile input. The fixture has no database connection.

Browser execution and exact-head CI are still pending. Local Chromium setup
failed to obtain a usable browser archive, so test discovery is not a browser
pass. This feature does not establish full rewrite or historical visual parity.
