# Catalog bump and image limits

[Issue #68](https://github.com/frankischilling/26chan/issues/68) adds the
limit indicators found in the pinned public catalog client v1025. Its card
renderer wraps `R: <b>count</b>` in `<i>` when `bumplimit` is set and wraps
`I: <b>count</b>` when `imagelimit` is set. It omits the image segment when
there are no image replies. The [reference manifest](public-catalog-reference.json)
records this observation and the unchanged client/CSS hashes. The pinned
public API documentation defines the two flags; it does not establish every
original counting or deletion rule.

The Rust catalog now renders those markers from its existing board snapshot.
Its [source-defined image rules](source-image-limits.md) differ from the JSON
interface for undead threads:

- The bump limit uses surviving replies, excluding deleted replies and the OP.
  Deletion can clear the marker. Sticky and permaage threads do not receive it.
- The image count excludes the OP image, deleted replies and deleted reply
  files. Deleting a reply file can clear the image-limit marker. Sticky and
  permaage suppress it everywhere; undead suppresses it in all public JSON,
  including catalog.json, but not in HTML catalog cards.
- Counts equal to or above the configured limit receive the marker. A zero
  bump limit prevents ordinary bumping immediately. Zero image capacity is
  already reached, including for retained attachments from an earlier policy.
  HTML still omits the image segment when the image-reply count is zero.

Board settings, surviving replies and visible attachment counts come from the
same repeatable-read transaction. Rendering performs no additional query or
state change. The markup retains the visible counts, their bold numerals,
the existing count tooltip and the existing thread navigation. All content
still passes through escaped Askama templates.

No database grant, migration, dependency, processing permission or production
enablement changes are included. This is one catalog behavior, not evidence
of complete original-page parity. The permitted client receives prepared
teaser data; its source does not establish original server-side preprocessing.

The supplied-source [bump rules](source-bump-rules.md) also apply the cutoff
after inserting the incoming reply. [Permaage/permasage controls](thread-bump-flags.md)
and [image-limit exclusions](source-image-limits.md) have separate verification.
Age/self-bump rules and full source parity remain unfinished.

## Earlier indicator qualification

The following records issue #68's implementation and tests before the source
count correction. Its lifetime-bump assertions are superseded by issue #114;
these historical results do not qualify the new write/count rules.

The following checks passed on Windows with the current owned PostgreSQL 16.15
cluster, after loading its private database environment:

```text
cargo fmt --all
cargo clippy -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo test -p board-public --all-features --locked --jobs 1 --quiet
cargo build -p board-public --example visual-fixtures --locked
node scripts/verify-public-catalog-reference.mjs .local/reference/catalog-20260913
node --check tests/themes/catalog-limits.spec.js
```

All 57 public tests passed. Added assertions exercise below/equal/above-limit
states through actual public-role routers, zero-capacity policy, policy changes
without new posts, lifetime versus visible counts, actual attachment creation,
file deletion and reply deletion. HTML markers and JSON flags agree after
each transition. The existing concurrent-commit test now compares pages that
also contain the limit marker, using its healthy blocked-reader witness.

The reference verifier validates all eight existing source pins, denies browser
network requests and measures normal/italic count styles under all six public
stylesheets at both widths. It passed all twelve combinations without executing
the original client JavaScript.

Six browser cases check the production template at desktop/mobile widths in
every theme, with JavaScript disabled. Their fixtures cover reached and exceeded
limits, deleted replies, no replies and disabled image capacity. The first run
passed the style assertions but failed on two missing screenshot baselines.
Both generated captures were individually inspected before acceptance. No prior
baseline, browser pin, retry count or pixel tolerance changed.

The complete sequential regression run passed all 71 fixture scenarios,
including 47 screenshot comparisons, and all ten actual-server behavior cases:

```text
npm run test:themes
npm run test:visual
npm run test:archive-visual
npm run test:media-visual
npm run test:states
npm run test:behavior
cargo fmt --all -- --check
```

The first five commands used `VISUAL_FIXTURE_SERVER=1`; the behavior suite used
the real application after clearing that variable and loading the current
database environment. All 45 previous baselines stayed unchanged. Source review
covered the snapshot-derived predicates, template arguments and escaping,
reference interpretation, policy/deletion assertions and browser fixtures.
No blocking finding remained.

Exact-head hosted CI is still required before merge. Native Linux qualifications
and deployed production evidence remain separate from these local checks.
