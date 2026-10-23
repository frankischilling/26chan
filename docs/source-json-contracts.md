# Source JSON contracts

Issue #216 tracks the complete JSON and navigation contract. These checks cover
directory policy, feed composition, conditional thread state and retained mobile markers. They do
not establish every generator or close the issue.

## Reference boundaries

The extractors pin `imgboard.php`, `json.php` and the INI parser from the supplied
source. The directory extractor evaluates only selected public configuration
keys and rejects interpolated values. Its flag selector comes from the separately
qualified source flag tables. It executes `get_catalog_info` for all 82 board
configurations. Private configuration values, the old application entry point,
network access and the source database are not needed.

`rebuild_boards_json` obtains board names and titles from a database absent from
the supplied source. That database is not reconstructed by these fixtures.
Existing inventory checks still qualify the available board definitions.

The post-state extractor executes the unchanged public post and tail serializers
with the original single-thread cache-state calculation. Its 288 state cases
cover counts immediately below, at and above the bump/image limits, sticky and
permaage suppression, undead image behavior, closed state and empty contexts.
Another 24 cases exercise file deletion, mobile presence and public membership
year filtering. Twenty-seven upload-board subjects qualify the source tag names,
leading-zero tag codes and empty titles. Persisted HTTP tests keep those tags on
opening posts only; replies retain their literal subjects. This does not enable
Flash uploads or claim completion of issue #206. Twelve cases with known attachment extensions are compared with
the Rust attachment serializer. A missing historical attachment cannot establish
that a companion existed.

Comments in this corpus are already-formatted plain text; the link helper is an
identity function, and semantic context is supplied. Identity and flag helpers
are disabled. Their separate fixtures remain necessary. The private banskip path
is never invoked. CI replays the captured, hash-pinned function slices without
requiring the original source tree.

## Feed composition

A separate extractor executes the original index, catalog, thread-list, archive,
full-thread and tail generators over 48 controlled caches. It supplies the removed
PHP 5 `each()` helper for PHP 8 and captures file output in memory. SQL and cache
population are explicit boundaries; no original database or filesystem output
is used. Cases cover page boundaries, page caps, sticky previews, zero preview
replies, naturally sorted reply IDs, deleted files and omission counts.

The HTTP test reproduces 24 text-only caches in a disposable database, then
compares both listeners with the executed outputs. It checks index, catalog,
thread-list, archive, full-thread and eligible tail responses. Database-generated
post IDs are mapped back to fixture IDs. Clock strings, unique-address counts
and helper-supplied tail eligibility are excluded from this comparison and keep
their existing dedicated tests. Empty threads reject ineligible tail requests.
Image-bearing cache cases remain source-execution evidence; existing approved
attachment tests cover their Rust projections and upload authority.

## Directory policy

Both public listeners expose configured capacities even when media processing is
not configured. `text_only` follows the stored board switch. JSON consumers must
not interpret these fields as processor readiness or permission to upload.
Actual route and attachment authority checks are unchanged.

Imported descriptions retain source entity spelling and trailing whitespace.
The lookup applies only when both board slug and current logical description
match the imported default. Operator edits remain literal text. This lookup
supplies JSON strings, never trusted HTML.

The restricted-role integration test compares complete metadata for the 80
public imported boards on both listeners, with and without media configuration.
It checks the two excluded boards, ordering, literal operator edits, denied
policy writes, escaped HTML and the absent unconfigured upload route.

## Thread state and file deletion

Full opening posts include zero-valued bump/image limit fields. Sticky posts
omit both. Permaage suppresses the bump field but leaves a zero image-limit
field. Tail summaries always include both numeric fields. Empty semantic context
remains an empty string in full opening posts and is absent from tail summaries.

The source keeps `m_img` after file-only deletion. Migration 0247 preserves that
marker while the associated asset remains approved. It does not restore the
file, expose a URL or grant media-reader access. Original, thumbnail and mobile
routes must still return 404 after deletion. Revoked assets and deleted posts
remain excluded. Legacy records without approved companion evidence are not
assigned a marker.

## Reproduction

```sh
python3 scripts/extract-json-directory-reference.py --fixture-only --check --php php
python3 scripts/extract-json-post-state-reference.py --check --php php
python3 scripts/extract-json-feed-reference.py --check --php php
cargo test -p board-public --lib api::
cargo test -p board-public --features database-tests --test json_directory_contract --test json_feed_contract
cargo test -p board-media-http --features database-tests --test still
```

The integration commands require the disposable migrated database and restricted
role credentials used by the repository test harness. Apply migration 0247 with
the matching binary. Browser and complete CI results must be checked on the final
PR head before merging.
