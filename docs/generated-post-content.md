# Generated posting content

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
Generated dice and fortune results now pass through content admission, board
wordfilters and the saved formatting pipeline. The remaining policy-hook work
is tracked in that issue; this change does not complete it.

## Posting order

In source revision `545b7812d1849f7958d914950c91fdbbe38f6b22`,
`imgboard.php:5548–5593` adds fortune markup after the comment or dice markup
before it. This happens after raw, repeated-line and line-count checks, and
before `spam_filter_post_content_new` and final blank-content admission.

The Rust caller generates one result from the locked board policy. The content
hook receives its fixed wrappers around escaped prepared user text, before
wordfilters. Final admission treats the same validated result as nonblank
content; raw bounds, required subjects and the text-only OP subject rule still
apply. The result retained in post metadata is the result used by the rule,
with no second random draw.

The formatter applies the selected board transformation to the combined
comment and generated wrappers before parsing finite markup. Test's two choices
are sampled once and saved with the payload. Reading an existing post never
samples them again or applies the board's current filter policy.

The wrapper helper accepts only a valid generated dice expression or an exact
fortune/palette pair. Comparison strings are never template-safe HTML. Forms
permit a blank comment when the board enables either randomizer; the server
still rejects an empty post without a valid generated result, subject-only OP
or authorized attachment.

## Saved formatting and rendering

Payload versions WF03 and WF04 retain finite generated delimiters alongside
ordinary text and markup. WF04 retains the existing staff-authorized limit
semantics; it does not grant authority by itself. Generated format ranges
488–495 and 504–511 distinguish these payloads from historical rows. A mismatched
format, missing payload or malformed payload does not fall back to trusting
stored HTML.

New-format readers include generated content in the saved comment and suppress
the separate historical dice/fortune presentation. Historical posts keep their
existing rendering. The same formatted lines feed public and staff templates,
JSON, RSS, search excerpts and catalog teasers.

HTML pages use a finite CSS palette instead of inline styles. Test may change
a source tag or attribute spelling; valid inert spellings have exact recipes,
and invalid digit-leading openings remain escaped text. JSON and RSS retain
the corresponding source projection. Native updater and quote-preview parsing
accept only the finite generated recipes, without allowing arbitrary styles,
classes or attributes.

## Storage

Migration `0176_generated_post_content.sql` extends the deferred empty-comment
guard to recognize retained dice or fortune metadata. It reads the final row
and also runs when generated metadata changes. Clearing a generated-only post's
last content fails at commit.

Migration `0177_generated_comment_format.sql` admits the new payload versions
and format ranges, updates the existing stamp and issuance functions, and
requires generated payloads to retain generated metadata. Existing role and
receipt checks remain in place. Apply both migrations before deploying the new
writer.

The populated upgrade fixtures preserve historical posts, metadata and audit
rows. They check function ownership and grants, trigger coverage, stamp
combinations and transfer-catalog readiness. These migrations do not convert
historical comments to the new representation.

## Qualification

Local qualification uses standalone manifests pointing at the production
sources, with SQLx TLS disabled only for an owned PostgreSQL 16 TCP fixture.
The populated upgrade through migration 0177 passes, followed by 12
content-admission, one randomizer and 14 staff-attachment database tests.

The actual HTTP test covers 48 generated-only OP/reply submissions. With filters
disabled it exercises both posting routes, form encodings and response modes;
additional Global and Test cases cover both route/encoding/response extremes.
It checks the saved payload, exact JSON projection and preservation after board
filter and randomizer settings change. Form checks cover enabled and disabled
randomizer policy.

Domain fixtures compare generated composition against 706 independently
recorded PHP results. A public rendering test covers all 36 Test choice pairs
for each of the 13 fortune colors, checking source JSON and the finite HTML
palette. The complete domain suite passes 290 tests; domain and public Clippy pass with
warnings denied. A focused staff-template harness compiles the production macro
and checks generated dice rendering. It is not a full staff application build.

All generated bundles match their pinned sources. The native filter bundle stays
within its existing 256 KiB budget by extending the guarded minification of two
private parser methods; exported names and application properties are unchanged.
The focused client run passes 69 tests with one test file running at a time. An
earlier concurrent run during Rust compilation hit four worker timeouts; no
request budget or timeout was raised. These local checks do not replace full
production CI.

A public-library run passed 182 tests; its Unix peer-credential test could not
create its socket because this environment returned EPERM. A broader client
run previously passed 900 tests and failed 129 where the Playwright executable
was missing. Neither result qualifies those unavailable runtime paths. Full
locked-workspace, native Windows and browser checks remain required before a
complete #212 PR can merge.
