# Staff JSON qualification

This checkpoint covers [source staff JSON projections](source-staff-json.md),
the independent meta-board setting and the wordfilter browser readiness fix.
It extends draft PR #227; every original board and feature remains the
completion scope. Issue #216 and the other full-scope issues stay open.

The supplied source revision is
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The JSON extractor pins
`json.php` at SHA-256
`18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868`.
It evaluates bounded pure blocks under PHP 8.3.6 with a 16 MiB memory limit,
five-second process deadline, 32 KiB output bound and 64 KiB fixture bound.
The fixture has 108 identity cases and eight reply-group cases.

The board extractor now records `META_BOARD` separately from `JANITOR_BOARD`.
All 82 supplied definitions leave meta presentation off. Removing only those
two added fixture fields reproduces the preceding board fixture exactly,
including every captured file hash. Regeneration checks also retain the exact
historical migration 0045 bytes and its additive encoding correction.

Migration 0072 has SHA-384:

```text
f58d1931f62cf1908a2a4e17f231f5320e89317c659084c23c40c1aff19df341367a8884e3998bf137134fd1f9f80a76
```

The separate owned qualification database applies all 63 migration files
through version 72. Earlier databases and role environments are retained.
Migration 0071 and its existing ledger checksum are unchanged. Migration 0072
adds one boolean column with a false default; it changes no function, owner,
grant or row-level privacy policy. Upgrade the schema before restarting the
new binary. Public readiness requires the column. Operators may enable meta
presentation independently of private access; runtime credentials cannot
change either policy.

## Executed checks

| Check | Result |
|---|---|
| Independent JSON reference regeneration | Passed; 108 identity and eight group cases |
| Board reference and historical SQL regeneration | Passed; all 82 definitions |
| Fresh role bootstrap and populated 0072 upgrade | Passed; previous board fields, privacy policies, owners and ACLs preserved; seven runtime mutation denials |
| Full Linux Rust workspace | 704 passed across 173 groups; no failures or ignored tests |
| Native Windows domain suite | 148 passed across 18 groups; no failures or ignored tests |
| Strict workspace Clippy | Passed; all targets/features with warnings denied |
| Workspace binary/example build and formatting | Passed |
| Generated asset guards | Passed; existing size limits retained |
| Client unit/DOM regressions | 109 passed; no failures, skipped or cancelled cases |
| Public browser regressions | 141 passed in 10.6 minutes; no retries |
| Separate staff browser flow | Passed after correcting the source JSON assertion; 11.3-second flow, no retries |
| Hosted checks at `d19f471` | Passed; build/test, monitoring and dependency advisories |

At `d19f4717018b77f9951e05608b565e31ecdc0c6f`, hosted
[build/test](https://github.com/frankischilling/26chan/actions/runs/37104237473),
[monitoring](https://github.com/frankischilling/26chan/actions/runs/37104237440)
and [dependency advisories](https://github.com/frankischilling/26chan/actions/runs/37104237474)
all passed. Build/test includes Linux Rust/browser checks, Windows visuals and
media/operations. These results qualify that checkpoint, not later changes.

The new persisted HTTP test passes within the full workspace. It checks all six
badges and three identity shapes across every forced-anonymous/meta combination,
both JSON listeners, full/tail/index/catalog/archive projections, deleted and
omitted replies, empty groups and stored-field preservation. A 1,001-reply
fixture requires the meta preview reader to fail before transferring badge
headers. Both routers and direct public-role queries remain unable to read
the fixture after it becomes private.

The new domain cases retain the literal JSON `admin_hl` comparison and the
distinct catalog `admin_highlight` predicate. They verify source grouping order,
highlighted-admin grouping and the existing 1,000-reply bound. Prepared trip
hashes, private trip passwords, public authority and source badge permissions
retain the independently qualified behavior from the staff identity checkpoint.

## Retained failures and scope

The preceding `0b0413c` hosted build/test run failed in a mobile wordfilter
browser case; its Windows visuals, media/operations, monitoring and advisories
passed. The retained trace shows a post-number click before Quick Reply finished
loading. The test now requires the native control to be attached before that
click. All ten local wordfilter cases passed after this correction. No timeout,
retry, screenshot baseline or generated bundle bound changed.

The first JSON HTTP fixture attempted to store both flag kinds, and the existing
paired-field constraint rejected it. The fixture now tests each valid flag kind
separately; the constraint is unchanged. The second attempt correctly received
404 after setting private access, which exposed the missing independent meta
switch. The new migration separates those settings and retains access denial.
The first expanded bootstrap check retained its temporary owner role beyond
the new comparison and could not call the existing poster-count function.
Scoping that comparison in a rolled-back transaction preserves the prior
checks and grants. Original failure logs are retained.

The first staff browser run expected highlighted-administrator names and trips
in forced-anonymous JSON. The independent fixture establishes that the literal
JSON exception is `admin_hl`, while saved `admin_highlight` remains visible in
the HTML catalog. The corrected browser assertions require masked JSON and
retain the catalog name/trip checks. Its original failure context is retained.
The corrected flow passes enrollment, login, audited moderation, recovery,
logout and both distinct identity projections.

Complete source JSON, meta-board client behavior, static staff ID labels,
unbadged authorized posting, Pass benefits, specialized media and the other
remaining board/frontend features are unfinished. Hosted success at a later
head must be recorded separately; local results do not qualify a different
commit or the entire rewrite.
