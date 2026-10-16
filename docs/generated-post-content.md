# Generated posting content

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
This checkpoint covers generated dice and fortune input to content rules and
final blank admission. Board wordfilters and complete saved/search rendering
of generated content still need work.

## Posting order

In source revision `545b7812d1849f7958d914950c91fdbbe38f6b22`,
`imgboard.php:5548–5593` adds fortune markup after the comment or dice markup
before it. This happens after raw, repeated-line and line-count checks, and
before `spam_filter_post_content_new` and final blank-content admission.

The Rust caller generates one result from the locked board policy. The content
hook receives its fixed wrappers around escaped prepared user text. It does not
receive a rendered or wordfiltered comment. Final admission treats the same
validated result as nonblank content; raw bounds, required subjects and the
text-only OP subject rule still apply. The result retained in post metadata is
the result used by the rule, with no second random draw.

The wrapper helper accepts only a valid generated dice expression or an exact
fortune/palette pair. Comparison strings are never template-safe HTML. Existing
renderers continue to escape retained text. Forms permit a blank comment when
the board enables either randomizer; the server still rejects an empty post
without a valid generated result, subject-only OP or authorized attachment.

## Storage

Migration `0176_generated_post_content.sql` extends the deferred empty-comment
guard to recognize retained dice or fortune metadata. It reads the final row
and also runs when generated metadata changes. Clearing a generated-only post's
last content fails at commit. The existing board-policy stamp, paired metadata
constraint and length/control bounds remain active.

The migration preserves post, thread, board and audit rows. Its populated upgrade
fixture includes historical dice and fortune metadata. It checks the attachment
function owner and grants, deferred trigger column coverage, and the transfer
catalog before and after the change. Temporary schema and function grants are
revoked. Apply the migration before deploying the new writer; the earlier guard
rejects generated-only posts.

## Qualification

Local qualification uses standalone manifests pointing at the production
sources, with SQLx TLS disabled only for the owned PostgreSQL 16 TCP fixture.
It passes 287 domain tests, 12 content-admission database tests, the existing
randomizer database test, and the actual HTTP admission test. The HTTP test
covers 32 generated-only OP/reply combinations across both posting routes,
form encodings and response modes, plus ordinary-content controls. It verifies
that JSON contains the retained generated result once. Form checks cover both
enabled and disabled randomizer policy. Domain, store and public Clippy pass
with warnings denied. Generated bundles and 28 focused client tests pass.

The first database run exposed the old deferred attachment guard; the new
migration fixes that mismatch. An upgrade-readiness assertion initially used a
role without permission to call it; the fixture now uses the existing transfer
owner role without adding a grant. The expanded HTTP matrix also reached the
process-local request ceiling. Independent matrices now use independent router
instances; production limits are unchanged.

A broader client run finished with 900 passing and 129 failing tests. Browser
launch errors report a missing Playwright executable in this environment. That
run is not browser qualification. Full locked-workspace, native Windows and
browser checks remain required in CI before a complete #212 PR can merge.
