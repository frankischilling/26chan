# Source image-limit indicators

[Issue #118](https://github.com/frankischilling/26chan/issues/118) matches the
supplied source's image-limit predicates and their representation boundaries.

`imgboard.php:1047-1073` sets the cached flag when the image-reply count reaches
`MAX_IMGRES`, unless the thread is sticky, permaage or undead. `json.php:12-30`
uses that cached thread state. Its `generate_board_catalogue()` at lines
598-653 also calls `log_cache()` and `generate_thread_json()`, so `catalog.json`
uses the same undead exclusion as full/tail thread JSON and board-page JSON.

The HTML catalog is different. `catalog.php:5-44` builds its embedded data
through `catalog_thread()`. The predicate at lines 149-152 excludes sticky and
permaage but does not exclude undead. Thus an ordinary undead thread at the
limit has an italic image count in HTML catalog cards and no true image-limit
flag in the public JSON API. The two generators must not be conflated.

Permasage does not independently change either image predicate. A zero limit
is already reached, even with zero images. Full/board/catalog JSON omits false
flags; tail JSON emits integer zero or one. HTML still omits the entire image
count segment when there are no image replies.

## Counts and authority

Counts come from the existing coherent snapshots and exclude the OP image,
deleted replies and deleted reply files. Rendering reads the persisted sticky,
permaage and undead states; it does not make extra database queries. `undead`
was added by migration 0020 and is available in the visible-thread projection
refreshed by migration 0022. Deploy that migration before this binary, as
described in [thread bump flags](thread-bump-flags.md).

This change adds no migration, grant, dependency or processing capability.
Public/staff mutation of undead remains outside their existing SQL grants.
Suppressing an indicator does not bypass image admission: the supplied
`imgboard.php:5051` checks admission separately from these flags. Existing
bounded, approved-attachment requirements and image-slot enforcement remain.

## Verification

The domain test covers all sticky/permaage/undead combinations, zero and
boundary limits, and large counts. All 19 domain tests and 35 public library
tests pass locally. Public/domain/store all-target/all-feature Clippy passes
with denied warnings after fixing a collapsible conditional in the new test.

The persisted attachment regression exercises all 16 combinations including
permasage through full/tail thread JSON, board JSON, catalog JSON and catalog
HTML. It checks unchanged counts, exact flag types/omission, internal-field
absence, body ETag changes, and permasage's unchanged full response. Existing
deletion and policy transitions remain, with zero capacity now expecting a
reached limit. A separate text-only thread checks zero images at zero capacity
through all five representations; HTML omits its image segment.

The existing actual-public-role attachment race now runs with sticky,
permaage and undead enabled. Exactly one of two approved capabilities can
consume the final slot, and the loser remains rejected until a file is deleted.
Both ordinary-comment and attachment-only variants retain their prior checks.

Six-theme desktop/mobile browser assertions add sticky/permaage/undead cards
and zero-capacity retained images. All 118 theme, 39 media/interaction, ten
public-state and three fixture-backed base visual cases passed without baseline
updates. Rust formatting, browser-script syntax and whitespace checks passed. Local
PostgreSQL is not listening locally. Both complete Linux runs 34811781867 and
34811775495 passed persisted, concurrency and native checks before PR #119
merged with the tested tree unchanged. These indicator rules do not
establish full media-format, source-page or production deployment parity.
