# Compatibility target

The pinned input is the public [4chan read-only API documentation](https://github.com/4chan/4chan-API/tree/2bd670d507ba2daa37a3961a661e088cf6f89d57), revision `2bd670d507ba2daa37a3961a661e088cf6f89d57`, collected September 8, 2026. [reference-manifest.json](reference-manifest.json) records exact source URLs and SHA-256 hashes. Files were fetched as public documentation; no live user posts, media or private source were imported. The input does not specify posting, authentication or private moderation behavior.

The official public [posting-options FAQ](https://www.4chan.org/faq#nonoko) is also pinned by collection time and response hash in the manifest. It supplies documented option meanings; no live external posting experiment was performed. See [posting-options verification](verification-posting-options.md).

A partial [public theme reference](public-theme-reference.json) pins six versioned stylesheets, two UI gradients, the public client script and observed selector/default metadata from September 13, 2026. [Desktop post-layout facts](public-post-layout-reference.json) and [catalog-card facts](public-catalog-reference.json) add computed styles on synthetic DOM and structural public-page observations; they are not original rendered-page screenshots. The earlier public-reference checkpoint did not use the supplied `4chan-old` checkout. At the user's subsequent request, the September 13 [supplied-source audit](#supplied-old-source-audit) inspected its static server, client, configuration and layout files. [Source inventory and declarations](#source-inventory) records inspected-file SHA-256 hashes, public setting declarations and version boundaries; no old runtime was executed or source code imported into the rewrite. No full original rendered-page snapshot or live posting observation has been collected. No independent design-brief attachment was available. Original source rules are now distinguished from missing rendered/deployment evidence and unfinished matching. Paperboard is not presented as an official service.

Evidence classes: **documented** means established by the pinned API docs or selected FAQ sections; **observed** means inspected public UI assets/metadata; **source** means established by a cited static branch or declaration in the supplied old checkout; **project** means a deliberate local behavior; **unknown** means a specific missing dependency, configuration, data set or observation prevents a conclusion. Source evidence describes this checkout, not necessarily the current live service. An implementation gap is not an unknown original rule. Status "implemented" applies only to the stated local scope, not universal client compatibility.

The source audit covers every compatibility and exception ID below. Its old configuration declares core v1123, extension v1178, catalog v1024 and desktop CSS v715; the separately pinned public reference includes core v1128, extension v1191, catalog v1025 and CSS v716. These inputs are not treated as one identical release. See [coverage and 1:1 targets](#compatibility-coverage) and [remaining unknowns](#remaining-unknowns). Existing tests and historical verification results are not new source-parity evidence.

[Posting-field measurements](public-form-reference.json) establish selected desktop geometry. The supplied template and handler now establish original OP/reply structure, desktop toggle/noscript behavior, multipart submission and conditional captcha controls. Matching the full rendered form/client flow and operating equivalent external captcha services remain unverified; the source-known flow is recorded under E-010/E-011.

| ID | Scope and evidence | Status | Tests / exception |
|---|---|---|---|
| I-001 | `/boards.json`; documented `Boards.md`; **source**: [original rules](#configuration-and-board-settings) | Partial: required settings, integer switches, text-only flag; character limits agree with posting | Public HTTP API and Unicode form/browser tests; development attachment settings in M-007; source-known original text/cooldown branches below; ambient encoding and external policy remain unresolved |
| I-002 | `/{board}/thread/{id}.json` and `/{id}-tail.json`; documented `Threads.md`; **source**: [original rules](#read-only-api) | Implemented text-post subset and development normalized media fields; full/tail support in #90 | `public_flow` and [thread snapshot regression](verification-thread-snapshots.md); numeric `no/resto/time`, escaped `com`, OP reply/image counts, conditional subject/sticky/closed/bump flags; archive OP fields and M-007 media metadata; [tail thresholds, counts and boundary](native-updater-tail.md); no unique-poster count, capcodes, trips, flags |
| I-003 | `/{board}/threads.json`; documented `Threadlist.md`; **source**: [original rules](#read-only-api) | Implemented page groups and visible-reply counts | One response snapshot; concurrent-commit regression; SQL aggregates avoid loading comment bodies |
| I-004 | `/{board}/{page}.json`; documented `Indexes.md`; **source**: [original rules](#read-only-api) | Implemented OP + latest five replies, omission counts | One response snapshot; concurrent-commit regression; positive 1-based JSON pages |
| I-005 | `/{board}/catalog.json`; documented `Catalog.md`; **source**: [original rules](#read-only-api) | Implemented OP + latest five replies, modification time | One response snapshot; concurrent-commit regression; M-007 normalized attachment metadata |
| I-006 | `/{board}/archive.json`; documented `Archive.md`; **source**: [original rules](#archives-and-retention) | Implemented for enabled boards, with automatic rollover, read-only archived threads and expiry | Optional per-board retention/count policy; actual JSON/HTML/cache/CORS and lifecycle tests in [archive verification](verification-thread-archives.md) |
| I-007 | Conditional JSON responses; documented API guidance; **source**: [original rules](#http-and-deployment) | ETag implemented everywhere; Last-Modified/If-Modified-Since on individual threads | Cache unit/integration tests; board and thread responses use one snapshot; board-only bump-limit changes invalidate thread body ETags; deletion invalidates list/catalog/index validators; same-second date requests conservatively revalidate |
| I-008 | Public HTML routes and DOM IDs; project plus pinned navigation/menu/filter observations; **source**: [original rules](#layout-and-themes) | Board index, zero-based numbered HTML pages, thread/post navigation, catalog, `t/pc/p/pi/m` IDs and native filter controls | Board/catalog/thread snapshot regressions plus browser behavior and visual tests; [thread navigation reference](public-watcher-navigation-reference.json) covers Return/Catalog, top/bottom anchors, mobile refresh and watcher placement. [Post-menu watching](thread-watcher.md#board-and-thread-post-menu-watching) has persisted browser and six-theme desktop/mobile coverage. [Native filter editing and page effects](thread-watcher.md#native-filter-editor-and-page-effects) have owned persistence, conflict, cancellation, hostile-input and six-theme usability tests. Other native post-menu actions and full page placement remain unfinished; no claim these cover every client selector or establish full visual parity |
| I-009 | Legacy-looking posting endpoint; project; **source**: [original rules](#posting-and-text) | Rust `/{board}/imgboard.php` and `/post` support multipart/URL-encoded text posting and exact Accept JSON responses | [Multipart fields and native submission](posting-multipart.md) implement `regist`/`post`, `pwd` and empty file parts through real transactions; [JSON response contract](posting-json.md) covers committed OP/reply IDs and error envelopes. Original single-request file handling, identity/captcha and complete Quick Reply flow remain E-010/E-011 work |
| I-010 | CORS, redirects, status/header details; API README plus project decisions; **source**: [original rules](#http-and-deployment) | Optional JSON-only listener with board-origin CORS, GET/HEAD/OPTIONS and conditional headers; public routes retain 303 posting, 308 board slash and same-origin writes | `api_cors`, startup and real cross-origin Chromium tests; no credentialed CORS. Header exposure/error details and deployment-domain mapping are project-defined; see [API contract](api.md) |
| B-001 | Persistent thread creation/replies; project; **source**: [original rules](#posting-and-text) | Implemented | Real PostgreSQL and browser tests; no copied posting internals |
| B-002 | `sage`; documented FAQ meaning; source-known original bump/count rules; local lifetime limits remain project-defined; **source**: [original rules](#counts-bumping-and-admission) | Implemented, serialized per board | Posting-options and concurrent reply tests; lifetime counts do not decrease after deletion |
| B-003 | Board settings and thread limits; project; **source**: [original rules](#counts-bumping-and-admission) | Unicode scalar comment limits, independent UTF-8 byte ceiling, [100-byte public name/subject limits](public-field-limits.md), active-thread cap, reply/bump limits | Domain/store/HTTP/browser tests; full boards displace oldest nonsticky threads; optional archives retain read-only threads, otherwise soft deletion; migration 0021 preserves historical fields and deletion |
| B-004 | Deletion; project; **source**: [original rules](#deletion) | Argon2 password; OP deletion hides whole thread | Wrong credential/origin, absent store, persisted deletion tests; no staff identity involved |
| B-008 | Post-submit destination and `nonoko`/`nonokosage`; documented FAQ; **source**: [original rules](#posting-and-text) | Implemented for new threads and replies through both posting aliases | Actual database redirect/bump tests and JavaScript-disabled browser controls; existing 303 status and exact unlisted-value rejection remain project behavior |
| B-005 | Greentext, same-board/cross-board quotes and spoilers; documented FAQ syntax with project grammar; HTTP(S) links; **source**: [original rules](#formatting-and-quotes) | Nonrecursive typed nodes; cross-board slice merged in #55 after reviewed-head Linux/Windows and monitoring checks | [Cross-board quoting](cross-board-quotes.md); bounded properties, escaped public/JSON/staff rendering and actual no-JavaScript navigation/deletion test. Original server parser rules are source-known; matching those rules and the complete inline extension remains unverified |
| B-006 | Reports; project; **source**: [original rules](#reports-and-staff) | Validated reasons persist; protected staff queue supports resolution and dismissal | Public reporting and staff database/HTTP/browser suites; original popup, weighted category queue and staff clearing are source-known; runtime category rows and operational policy are missing |
| B-007 | Staff moderation/authentication; project requirement; **source**: [original rules](#reports-and-staff) | Separate WebAuthn app, absolute and idle session expiry, recent-authentication/CSRF checks, audited close/sticky/removal actions and operator-controlled enrollment/revocation/recovery | Staff database/HTTP tests including concurrent expiry and virtual-authenticator browser flow; hardware authenticator, production policies and independent review remain unverified |
| M-001 | Media intake/publication; project security requirement; **source**: [original rules](#media) | Production uploads disabled; explicit loopback browser intake and separate development dispatch/publication implemented | Startup and unsupported-upload tests plus [owned native dispatch qualification](verification-media-dispatch.md); PNG and JPEG browser/guest flows passed native CI. [JPEG #51](jpeg-media.md) merged after complete checks; deployed production processing containment remains unverified |
| M-002 | Private quarantine and persisted queue; project; **source**: [original rules](#media) | Development operator and authenticated HTTP intake, admission limits, leases/retries and terminal cleanup implemented | `board-media` storage tests, `media_queue`, capability-scoped `media_intake` database tests and actual public multipart streaming tests |
| M-006 | Authenticated HTTP intake; project security requirement; **source**: [original rules](#media) | Separate restricted login, hashed reservation capability, exclusive upload claim, bounded streaming, private status/readiness and development service candidate | [Intake verification](verification-media-intake.md), including owned service/Firecracker/reader and live-capability restore qualification; development public attachments are covered in M-007. Production identity/network/storage policy remains unfinished |
| M-007 | Persisted post attachments; project security requirement and pinned API media fields; **source**: [original rules](#media) | Partial: development no-JavaScript forms, atomic one-use consumption, reply-image limits, escaped HTML, spoiler links, public/staff file-only deletion, normalized API metadata, PNG thumbnails and physical cleanup of deleted/expired or aged unused outputs | [Contract and tests](post-attachments.md); retention/browser cleanup and [populated restoration](attachment-restore.md) passed CI. Staff preview/action passed database, actual-reader browser and complete CI checks. [Offline legacy manifest upgrade](legacy-media-upgrade.md) and eight inspected [media visual baselines](verification-media-visuals.md) passed PR CI on `9a8e5b1`. [JPEG input](jpeg-media.md) merged after local and complete native checks on `6be7a57`. Production scheduling and original-site parity remain separate launch/reference requirements |
| M-003 | Bounded worker output and promotion; project; **source**: [original rules](#media) | Fixed RGBA stream/disk protocols, generated PNG, durable per-lease approval, idempotent publication, interrupted-output cleanup and restricted reader implemented | [Approval tests and operator workflow](media-approval.md); development attachments and HTTP serving are recorded in M-007/M-005. Power-loss qualification and production media-origin/domain deployment remain incomplete |
| M-004 | Per-job isolated execution; project security requirement; **source**: [original rules](#media) | Local Firecracker/jailer guest with Rust PNG/JPEG decoders, raw disks, bounded host validation, operator orphan recovery and authenticated queue/publication dispatch | Actual owned WSL [full dispatch and lease tests](verification-media-dispatch.md), [SIGKILL recovery tests](media-recovery.md) and [execution profile](firecracker.md). [JPEG native cases](jpeg-media.md) passed hosted Linux qualification before #51 merged. Production host qualification, deployed dispatch/certificate operations and complete network/storage controls remain incomplete; production uploads stay disabled |
| M-005 | Separate approved-media HTTP serving; project security requirement; **source**: [original rules](#media) | GET/HEAD of generated PNGs through a distinct reader app, approved-view login, checked bytes, conditional responses and an explicitly shared read-only store | [HTTP/browser and owned service qualification](verification-media-http.md); new [numeric full/thumbnail routes](post-attachments.md) follow the pinned URL shapes but serve only PNG. Opaque paths remain project-defined. Original downloads, other legacy formats and production DNS/TLS/domain/egress policy remain incomplete |
| V-001 | Desktop/mobile board, thread/form spacing and typography; partial observed theme and post-style evidence; **source**: [original rules](#layout-and-themes) | Referenced desktop post padding, comment spacing, thumbnail floats, reply arrows/borders and OP file order merged in #59 after complete checks; posting-field scope is V-004 | [Post-layout verification](public-post-layout.md), six computed-style browser cases and reviewed board/thread/theme captures; 1280x900 and 390x844, scale 1, locale en-US. Original templates and desktop/mobile CSS are source-known; full rendered-page geometry and client behavior remain unverified |
| V-002 | Media thumbnails/spoilers, archives, loading/error/empty snapshots; source-known templates/CSS; original rendered-state comparison missing; **source**: [original rules](#layout-and-themes) | Partial project regression coverage: six archive, eight attachment and ten public empty/error baselines; empty/error slice merged in #57 after complete exact-head checks | [Media visual verification](verification-media-visuals.md) and [public-state verification](verification-public-states.md); JS-disabled desktop/mobile layouts, correct empty-catalog navigation and actual 404/503 error rendering. Post-layout capture changes are tracked in #58. Comprehensive original reference screenshots and loading states remain unavailable |
| V-003 | Six named themes, base fonts/palettes, work-safe defaults and grouped persistence; observed public CSS/client metadata; **source**: [original rules](#layout-and-themes) | Merged in #53 after reviewed-head Linux/Windows, advisory and monitoring checks; 12 inspected theme baselines | [Theme reference and tests](public-themes.md); finite independent host-only cookies, no-JavaScript forms, private CSS, fixed local gradients and actual-browser CSP positive/negative control. Original selectors/style grouping are source-known; full matching and rendered-page comparison remain unverified; E-009 |
| V-004 | Desktop posting-field geometry; observed public CSS/DOM with explicit local control differences; **source**: [original rules](#posting-and-text) | Shared normal/approved-image fields implement measured table spacing, widths, label styling and textarea geometry; merged in #61 after complete exact-head checks | [Form verification](public-posting-form.md); six style/mobile-editability cases, actual public/approved-image posting, and reviewed desktop/mobile captures. Script-free expanded form, password/options policy and mobile sizing remain local behavior; E-011 |
| V-005 | Default extended-small catalog cards; observed public v705 CSS and v1025 client structure; **source**: [original rules](#catalog-teasers-search-and-spoilers) | Compact cards, bounded thumbnails opening threads, escaped subject/teaser and visible reply/image-reply counts; merged in #63 after complete exact-head checks | [Catalog verification](public-catalog-cards.md); six desktop/mobile style/navigation cases, real deletion and coherent snapshot counts, five reviewed captures. Controls are V-006 and fallback graphics are V-007; later menus/search fields are V-009. Original teaser processing is source-known; matching its board-dependent pipeline remains incomplete |
| V-006 | Catalog sort, size, teaser and quick-filter options; observed public controls/client and CSS; **source**: [original rules](#catalog-teasers-search-and-spoilers) | Four sorts use visible snapshot state; small/large and teaser on/off modes plus search/reset work without JavaScript. Initial controls merged in #65; subsequent search behavior is V-009 | [Control verification](catalog-controls.md); actual-role deletion/sorting tests, concurrent-commit queries, persisted sage browser workflow, all-mode six-theme properties and six additional captures. GET submission/URL persistence and toolbar wrapping are explicit local behavior |
| V-007 | Catalog no-file, deleted-file, generic spoiler and sticky/closed icons; observed public v1025 client and v705 CSS with pinned public images; **source**: [original rules](#catalog-teasers-search-and-spoilers) | Seven fixed image routes and measured state geometry implemented; merged in #67 after all exact-head checks passed | [Asset verification](catalog-state-assets.md); GET/HEAD hashes, MIME/cache/CSP and denied writes, six-theme desktop/mobile geometry at scale 1/2, real browser positive/negative CSP controls, hidden-media non-fetching and nine reviewed captures. Board-specific spoilers, reveal preferences and full-page parity remain incomplete |
| V-008 | Catalog bump/image-limit indicators; observed public v1025 card markup and documented API flags; **source**: [original rules](#counts-bumping-and-admission) | Italic R/I counts use the coherent board snapshot and existing JSON rules; merged in #69 after all exact-head checks passed | [Limit verification](catalog-limits.md); actual-role boundary/deletion/policy transitions, concurrent snapshots, six-theme desktop/mobile computed styles and two inspected captures. Original visible-count/deletion rules are source-known; lifetime bump counting differs, and complete catalog matching remains unverified |

Later compatibility checkpoints:

| ID | Scope and evidence | Status | Tests / exception |
|---|---|---|---|
| V-009 | Browser-local catalog display, search/session and pin/hide behavior; observed pinned public v1025 client; **source**: [original rules](#catalog-teasers-search-and-spoilers) | Display persistence, in-place controls, bounded search operators/case handling, live/session search, thread menus and pin/hide state, and shared serialized search fields merged through #83 after exact-head checks | [Preferences](catalog-preferences.md), [in-place controls](catalog-inplace.md), [operators](catalog-search.md), [live search](catalog-live-search.md), [pin/hide #81](https://github.com/frankischilling/26chan/pull/81), and [search fields](catalog-search-fields.md). Matching the source-known formatting, Unicode whitespace, filename and truncation pipeline remains open in #82; watchlist work is tracked in V-011; complete native options remain unfinished |
| V-010 | Catalog spoiler-reveal preference; observed pinned public v1025 client; **source**: [original rules](#catalog-teasers-search-and-spoilers) | Merged in #87 on September 13, 2026 | [Spoiler behavior and tests](catalog-spoilers.md); finite optional persistence, explicit no-JavaScript GET, visible-card-only client source changes and deletion precedence. Toolbar/URL extension and full-image fallback are documented differences; custom board spoiler selection is source-known; matching board-specific assets remains unfinished |
| M-008 | Attachment-only OP/reply and conditional comment representation; documented API field plus local posting policy; **source**: [original rules](#posting-and-text) | Merged in #85 after all six exact-head checks; ordinary text-only posts still require a comment | [Attachment-only contract](attachment-only-posts.md); atomic authorization, deferred empty-row guard, direct public-role rejection, 0018 upgrade, real HTTP/no-JavaScript and native PNG/JPEG flows. Original image-only replies are permitted, but an ordinary OP still needs subject or comment. Empty-OP and whitespace-only rejection policies remain local differences; no production enablement |

### Native extension checkpoint under draft PR #89

| ID | Scope and evidence | Status | Tests / exception |
|---|---|---|---|
| V-011 | Native watcher, post menus, ordinary reply/thread hiding, menu-ready event, optional shortcuts and manual/automatic in-place updating; pinned public catalog v1025 and extension v1191; **source**: [original rules](#native-extension) | Merged in [#89](https://github.com/frankischilling/26chan/pull/89) after all seven exact-head checks passed; broader reference qualification remains open | [Watcher](thread-watcher.md), [thread hiding](native-thread-hiding-state.md), [menu event / recursive-helper reachability](native-post-menu-events.md), [keyboard integration](native-keyboard-shortcuts.md), and [thread updater](native-thread-updater.md). Update/R and Auto/A use bounded snapshots; [full/tail selection and conditional revalidation](native-updater-tail.md) are implemented in #90, with validated DOM construction, native events and existing menu/filter/watch integration. Auto adds per-tab state, backoff, unread title/marker, fixed favicon/sound notifications and the pinned hidden-tab scroll rule. Posting receipts decorate tracked quotes; notification priority waits for page filters. Quick Reply coordination is tracked in V-012; live non-worksafe board observation and complete public-page parity remain unfinished. Recursive helper definitions are not treated as evidence of an exposed built-in recursive menu |
| V-012 | Quick Reply text posting, quoting, persistence, cancellation and updater coordination; supplied old extension source | Implemented in #100; local transport/theme/fixture checks passed, persisted-browser checks await CI | [Quick Reply](native-quick-reply.md) records bounded transport, source guards and editing, exact IDs, current-board CSP, tests and remaining cooldown/identity/captcha/drawing/inline-file and rendered-source work. E-010/E-011 remain explicit security replacements |

## Security-driven and project-defined exceptions

Catalog display preferences now have bounded browser-local persistence; see
[preference behavior and remaining differences](catalog-preferences.md).
In-place display controls are covered by [snapshot and rendering checks](catalog-inplace.md).
Live search and session restoration are covered by [live-search checks](catalog-live-search.md).
Pin/hide state and thread menus are implemented in V-009; watcher/menu/updater work is tracked in V-011. The complete native options panel and full-page parity remain unfinished. Their original source behavior is recorded in the source audit.

The [catalog search operator and case contract](catalog-search.md) now follows the
pinned client rather than treating every punctuation character literally. The
subject/teaser fields now share a [bounded serialized representation](catalog-search-fields.md).
Full formatting, whitespace, filename and truncation fidelity remains a separate
open gap in #82; these tests do not establish universal native search parity.

Public request budgets are project-defined containment policy, not claimed reference behavior.
Their existing defaults and bounded operator overrides are documented in
[public request limits](public-request-limits.md).

| ID | Reference/old behavior | Replacement and reason | User impact and test |
|---|---|---|---|
| E-011 | Original template uses a desktop toggle-hidden post table, a noscript reveal, conditional external captcha, inline file/spoiler/text-only controls and OP/reply hidden fields; source-known; [source detail](#posting-and-text) | Expanded server-rendered fields retain the script-free CSP, explicit required deletion password and separate isolated-upload workflow | Core posting needs no script or external widget; labels and focus outlines remain available, and narrow screens use 16px fields. Actual posting/validation/browser checks cover this local flow. No equivalent captcha is implemented or claimed; production abuse protection and complete form behavior remain unresolved. |
| E-009 | Original client selects ws_style/nws_style cookies and JavaScript stylesheets; category defaults and catalog selection are source-known; [source detail](#layout-and-themes) | Finite host-only HttpOnly preference cookies, same-origin server forms, private CSS and fixed local UI image paths: two gradients and the seven catalog assets in V-007 | Preserves the public script-free CSP and staff/media cookie separation. Style selection takes an extra page/apply action. HTTP and JavaScript-disabled browser tests verify independent groups, cookie flags, bounded inputs, origin checks, safe redirects and persistence; original control layout is not claimed. |
| E-001 | API formats are documented; source admission, cleanup and PDF/WebM/audio switches are board-dependent, not universal support for every listed extension; [source detail](#media) | Production uploads and all original-download paths remain disabled; the explicit development profile accepts bounded PNG/JPEG input and publishes only normalized PNGs | [JPEG policy and checks](jpeg-media.md): source metadata is discarded; EXIF orientation and ICC color management are not applied. Limits can reject otherwise viewable files. Other input formats and original downloads remain unsupported. Private development input expires through queue cleanup. Worker containment does not make downloaded files safe in every client parser. |
| E-008 | Original source generates JPEG thumbnail URLs and hashes the accepted file after configured metadata/chunk cleanup, before base64 API MD5 serialization; [source detail](#media) | The legacy-looking thumbnail route serves normalized PNG with truthful `image/png` and `nosniff`; full-file metadata and MD5 describe the normalized PNG | Retains one encoder and avoids another privileged image parser. Clients that require JPEG bytes at that route are not compatible. Actual byte, MIME, checksum, validator and deletion checks run in the browser test. Unknown legacy manifests omit missing fields until the isolated offline upgrade reproduces their exact approved PNG; original-upload checksum parity is not claimed. |
| E-010 | Original source posts a single multipart form with mode=regist and optional file; success is exact-Accept JSON tid/pid or an HTML meta-refresh flow; [source detail](#posting-and-text) | Development image posting uses upload, POST status check, then the normal post form after approval | Keeps pending drafts/passwords out of worker waits and commits no unapproved attachment. Extra user interaction and a two-hour receipt deadline are project security policy. Real no-JavaScript PNG/JPEG browser flows through the guest, publication, deletion and cleanup passed CI. |
| E-002 | Ordinary original text is escaped; only an authorized HTML branch with allow_html and role/flag checks uses a purifier; [source detail](#formatting-and-quotes) | User text becomes typed formatting nodes; Askama escapes every text/attribute | HTML appears as text. Unicode and real-browser hostile-text tests. |
| E-003 | Original cookie/password-based staff authentication, board-scoped roles and OTP hooks are source-known; deployed account/secret/recovery policy is not supplied; [source detail](#reports-and-staff) | Separate-origin staff WebAuthn with required user verification; operator-controlled invitations and recovery. Public author deletion still uses local Argon2 passwords | Staff requires a compatible authenticator and JavaScript. No password or recovery-code staff login. Virtual-authenticator and protected-schema tests; no hardware-attestation guarantee. |
| E-004 | Original active-thread unique_ips is a count of visible host values including the OP, conditionally exposed; archives clear those values; [source detail](#read-only-api) | Omitted; persistent IP tracking is not implemented | Clients expecting that field need an exception. No invented count. |
| E-005 | Original handler normalizes CRLF/CR to LF, then checks mb_strlen without an explicit encoding before later cleanup; UTF-8 counts code points, but deployed mbstring encoding is missing; [source detail](#posting-and-text) | New comments normalize CRLF and lone CR to LF before counting Unicode scalars and insertion; at most 16,000 characters and 64,000 input UTF-8 bytes. Default/production boards advertise zero media capacity and `text_only: 1`; enabled development boards advertise the bounded upload size and configured image limit | `é` and `😀` each count as one scalar; combining marks, emoji modifiers and joiners count separately. [Newline contract and coverage](verification-comment-limits.md#posting-newline-normalization) in #92. Later source cleanup and deployed encoding remain separate gaps. Existing stored comments are unchanged; this is still a partial settings contract. |
| E-006 | Original category/board cooldowns, duplicate checks, deletion timing/action thresholds, protected-thread rollover and MySQL/static-rebuild branches are source-known; external configuration and distributed/proxy behavior remain missing; [source detail](#counts-bumping-and-admission) | 30 writes per peer per minute, 32 admitted public/API handlers and retained response bodies/data, 4 Argon2 operations; active-thread rollover and all-pinned rejection follow B-003 | Local overload remains 503 (staff has a separate 16-request budget and 429). No bypass through forwarded headers. Router, response-ownership and concurrency tests; production byte budgets and transport timeouts remain unqualified. |
| E-007 | Original deletion hard-deletes board rows and unlinks assets, clears relevant reports, and may retain selected content in staff deletion logs; archives clear IP/password/email/Pass fields and expire by configured hours; backup/operational erasure policy is missing; [source detail](#archives-and-retention) | Deleted text remains in PostgreSQL but disappears from public routes; reports persist | Compromised public database credentials can read retained content. Operator retention/erasure policy is a launch prerequisite. |

The synthetic fixtures and screenshots are project-owned test data. Screenshot changes require review in the recorded browser/platform/font environment. Current baselines were generated from the database-backed application, visually inspected, then checked against the separate renderer using the same production templates.

Archive settings and lifecycle are specified in [archive notes](thread-archives.md). The source audit now establishes original protected-thread counting, rollover, credential/IP clearing, hour-based storage retention and the separate 72-hour/3,000-item HTML archive list. The rewrite's bounded count, fixed per-thread expiry, all-pinned rejection and soft-retention model remain explicit differences, not unknown original behavior. Six synthetic archive baselines do not establish original-site visual parity.

## Supplied old-source audit

### Reference boundary

On September 13, 2026, the user authorized static inspection of the supplied
`C:\Users\imike\4chan-rewrite\4chan-old` checkout to resolve the original-behavior
unknowns in the compatibility register above. This audit covers all 48
compatibility and exception IDs. It adds source evidence, not implementation,
test results, source-provenance certification or live-service observations.

[The source inventory](#source-inventory), [public setting declarations](#public-setting-declarations)
and [native defaults](#native-default-declarations) below record file fingerprints,
source line counts, board overrides and native boolean defaults. References below
are relative to the supplied checkout. The ignored old checkout is not copied
into the rewrite or required to run it. No PHP application, external service,
database import, media processor or test was executed for this audit. Secret
values, account records and production post data are not reproduced.

The source configuration declares core JavaScript 1123, extension 1178,
catalog 1024, desktop CSS 715 and catalog CSS 705
(`config/global_config.ini:3-17`). The public references separately pin newer
core 1128, extension 1191, catalog 1025 and desktop CSS 716. A declaration is
not proof that every supplied plain-text/minified asset belongs to that exact
release. Old-source facts must not silently overwrite a differing modern
public observation or be presented as current live behavior.

The earlier verification documents retain their historical source restrictions
and test results. Their new evidence notes point here rather than pretending
this inspection occurred during those checkpoints. The documentation commit
also leaves the separate in-progress notification work uncommitted.

### Configuration and board settings

`yotsuba_config.php:16-52` loads a board's declared category when present,
otherwise a subdomain group, then applies board overrides. Global declarations
are loaded separately. `lib/ini.php` supplies the custom parser, including
yes/no conversion, integer conversion, placeholders, external-file references
and random values. This is not equivalent to applying a native INI parser and
assuming the result is complete production configuration.

The public setting tables below record public overrides from all 82 supplied board configuration
files. Category/group inheritance is kept separate from board declarations.
It does not fabricate missing subdomain defaults, resolve external files, or
treat database board metadata as known. Examples: `a.config.ini` declares
180 archive hours, 500 replies and 300 reply images; `jp.config.ini` declares
5,000 comment characters, a 3,600-second new-thread cooldown and 250 archive
hours. These are snapshot declarations, not current live settings.

Global values include 2,000 comment characters, 10,000 authorized comment
characters, 300 replies, 150 reply images, five preview replies, 70 comment
lines and 276 archive hours. Worksafe category settings override the reply
limit to 310 and lines to 100. Non-worksafe category settings use 30-second
image cooldowns and 300 reply images. Ordinary reply and new-thread category
cooldowns are 60 and 600 seconds respectively; board declarations may override
them. Use the public setting declarations below instead of one universal invented limit.

Favicon mapping is explicit: `config/categories/ws.config.ini:11` uses
`image/favicon-ws.ico`; `nws.config.ini:6` uses `image/favicon.ico`.
`imgboard.php:3388` emits the configured favicon. The success-page branch
also selects those two icons (`imgboard.php:6828`). The denied modern
non-worksafe public request remains an uncaptured live observation, not an
unknown original mapping.

`imgboard.php:7846-7932` maps loaded settings into board metadata, including
cooldowns, file capacities, character/bump/image limits and conditional
spoilers, custom-spoiler counts, IDs, archives, code/SJIS/math tags, flags,
WebM audio, minimum image dimensions and text-only/required-subject switches.
Zero-media production boards in the rewrite remain deliberate replacements.

### Read-only API

`json.php:13-164` serializes OP/reply thread data and the reduced last-five
reply windows with omission counts. Tail JSON carries an OP summary,
`tail_size` and `tail_id`, while full-thread output carries tail hints.
`json.php:540-720` generates index/catalog page groups, thread listings
and archive IDs. Archive IDs are explicitly sorted by post number ascending;
the HTML archive list has a different order and time window.

`imgboard.php:850-1074` builds counts from existing SQL rows. Replies count
children, excluding the OP. Reply images require a file size and no file-deleted
flag. Index/catalog omission counts are therefore not lifetime counters.
`json.php:303-527` removes internal host, password, Pass and bookkeeping
fields, drops file metadata when absent/deleted, omits empty comment/subject
fields, and conditionally emits archive/sticky/closed/limit/identity fields.
MD5 is hex-to-base64 conversion of the accepted stored-file hash, not a
guarantee that the original incoming upload bytes were untouched.

Capcodes, extracted trip spans, IDs, country/board flags and authorized-reply
lists have source-known serialization branches
(`json.php:172-214, 459-513`). Their omission by the rewrite is a feature
gap, not an unknown API field meaning. Forced-anonymous and role/board settings
affect exposure; runtime account, flag and geolocation data are separate inputs.

`imgboard.php:9077` counts visible host values including the OP for active
threads. It does not invent a lifetime distinct-poster count, and archive
processing clears host values. The `SHOW_THREAD_UNIQUES` branch conditionally
exposes a positive `unique_ips` value. Omitting it in the rewrite remains
the explicit privacy/implementation exception E-004.

### Posting and text

`views/imgboard.php:45` defines one multipart POST with `mode=regist`,
hidden `pwd` and `MAX_FILE_SIZE`, and `resto` for replies. The template
supplies name/email/subject/comment fields plus conditional file, spoiler,
text-only, drawing and captcha controls. The desktop post table is
toggle-hidden; the server's noscript style reveals it. OP/reply structure
is available in the supplied source, not an unknown inferred from API docs.

The rewrite's [multipart text form](posting-multipart.md) uses the source
action, `mode=regist`, `pwd` and checkbox values, accepting empty browser file
parts. Both endpoints also retain URL-encoded support. `mode=post` is an
alias; other dispatch modes are rejected by the posting parser. The explicit
password control and isolated file workflow remain documented security
exceptions, not implementations of UserPwd or single-request raw-file posting.

New public names and subjects follow the source's 100 input-byte limit.
The shared form omits the source-absent `maxlength` attributes. See
[field limits](public-field-limits.md) for migration 0021, retained historical
subjects, database authority and the tested scope within B-003/V-004.

The dispatcher accepts `regist`/`post` on POST and calls `new_post`
(`imgboard.php:10295-10300`). The handler uses the UserPwd session token
for posting identity (`imgboard.php:4891`, `lib/userpwd.php:150, 401`).
The generated `4chan_pass` cookie has a one-year TTL and secure/HttpOnly
setter (`lib/userpwd.php:36-38, 930-934`). This is not the rewrite's
explicit Argon2 deletion-password model. Pass authentication in `auth.php`
and public email verification in `signin.php` are distinct from staff
authentication; their external account/key/service data are not supplied.

The normal text path has a specific order
(`imgboard.php:5295-5443, 5752-5842`):

- Normalize CRLF and bare CR into LF.
- Check `mb_strlen(com)` without an encoding argument against the normal or
  authorized board limit, before later normalization/cleanup.
- Check name/email/subject byte lengths with `strlen`; these are not the same
  Unicode-length checks as the comment field.
- Apply board-dependent normalization, zero-width/private-character/emoticon
  cleanup and whitespace-only handling; code/SJIS boards have exceptions.
- Remove intra-word spoiler tags in the dedicated branch and rewrite a
  same-board cross-quote to its ordinary same-board quote.
- Sanitize text, process name/trip/role and board requirements, convert ordinary
  comment newlines to breaks, parse enabled markup, linkify and insert wrapping.

With UTF-8 mbstring encoding, `mb_strlen` counts Unicode code points, not
grapheme clusters or UTF-16 units. The production default encoding is absent.
`lib/postfilter.php:130` temporarily selects UTF-8 during conversion and
restores the previous mbstring encoding; it does not establish the later
handler's ambient encoding. That specific environmental uncertainty remains.
The rewrite normalizes CRLF and bare CR before its bounded scalar/byte checks
for new posts. Its UTF-8 scalar policy and the remaining source cleanup rules
still need to be distinguished from the old handler's deployed environment.

`imgboard.php:5412-5424` detects `sage` anywhere case-insensitively in the
email/options field and removes all matching substrings. The remaining value,
lowercased, is compared exactly with `nonoko`. Thus original processing is
not the rewrite's finite exact options allowlist; the FAQ meanings remain
documented, while acceptance and response mechanics differ.

`imgboard.php:5789-5802` requires an ordinary OP to have a subject or
nonblank comment, even with an image; configured/authorized exceptions apply.
An image reply may have no comment; a fileless reply needs text. Text-only
OP policy also requires a subject. Allowing a normal file-only OP with neither
subject nor comment is a known local difference. Empty `com` omission
in JSON alone was never evidence of OP admission.

`imgboard.php:6815-6879` returns `tid`/`pid` JSON only for the exact
`Accept: application/json` branch. Otherwise it renders a success page with
meta-refresh, normally one second or ten seconds in the delayed branch,
and optional configured success content. The rewrite implements the exact
Accept JSON branch with `tid: 0` for an OP, the parent ID for a reply and
the inserted `pid`. The source's `error_json` branch at 3808-3816 returns an
HTTP 200 error envelope for posting-rule failures. The rewrite preserves
4xx parser/containment rejections and 5xx dependency failures;
[posting JSON](posting-json.md) records the contract and tests. Original
single-request upload/post sequencing and the HTML response flow are known.
The rewrite's 303 and upload/status/approval/post sequence remain E-010,
not unspecified old behavior. [Quick Reply](native-quick-reply.md) implements
text submission, persistent/cancelled drafts, source quote editing, own-post
tracking and updater coordination in #100; its complete lifecycle remains open.

### Formatting and quotes

`imgboard.php:7289-7328` escapes ordinary user text with
`htmlspecialchars(..., ENT_QUOTES)`. The HTML-allowed path additionally
requires `html == 1` and an authorized manager/HTML/developer role/flag
before purification. Do not generalize that branch into ordinary raw-HTML
acceptance or copy it into the public rewrite.

`parse_bbcode_one` and the spoiler/code/SJIS/OP-markup helpers
(`imgboard.php:501-620`) have nesting limits and unfinished-tag handling.
Spoilers become `s` elements; code, SJIS and colored/OP markup are
board-enabled branches. Later word wrapping inserts `wbr`, and greentext
matching is applied at line/break boundaries
(`imgboard.php:5752-5842`). Those source rules differ from typed,
nonrecursive local formatting nodes; they now have concrete 1:1 targets.

The active posting call is `normalize_and_linkify`
(`imgboard.php:5828`), using URL normalization, `auto_link` and quote
helpers (`imgboard.php:3877-4243`). The alternative
`auto_link_parser` definition at 4244 is not evidence that it runs in this
path. Same-thread links use fragments; other replies resolve their containing
thread; missing posts become dead links. Interboard links depend on valid
boards and post lookup, with explicit board-pair exceptions in the old helper.
Ordinary same-board cross-quote rewriting happens earlier in the handler.

Trip construction is visible at `imgboard.php:5469-5515`: ordinary trips
use salt conversion and `crypt`, secure trips use SHA-1/base64 with an
external salt. Poster ID generation is a separate board-dependent helper
at 4674. Their algorithms are not unspecified, but external salts and
dynamic board/host inputs cannot be recreated from a source declaration alone.
The rewrite still needs an implementation or explicit exclusion; private salts
must not be reproduced to claim parity.

### Counts, bumping and admission

Original bumping uses current SQL reply counts, not the rewrite's retained
lifetime reply total (`imgboard.php:6627-6653`). Sage, sticky,
permasage and permaage affect bumping through separate branches. OP self-bumps
are also controlled by the 900-second initial and 300-second later
declarations (`config/global_config.ini:232-237`,
`imgboard.php:5975`). The configured sage interval is not proof that
every similarly named constant is enforced in every path.

Reply-image admission counts existing non-file-deleted reply files and excludes
the OP (`imgboard.php:5045-5052`). Deleting a file can free capacity.
The API/cache limit flags use current reply/image counts with permaage,
sticky and undead exclusions (`imgboard.php:1036-1073`);
the HTML catalog uses its own permaage/sticky branch
(`catalog.php:149-152`). These slightly different branches must stay
distinct in a 1:1 implementation.

Active default/category cooldown checks run in
`imgboard.php:5866-5973`: duplicate comments/images, ordinary reply/image
cooldowns, new-thread cooldowns, cross-board recent-thread actions and
conditional Pass reductions. Normal reply/image cooldowns are not dead code;
the commented XFF alternative is separate. Defaults are declarations in the
public setting tables below, including category/board overrides. Conditional known-user, role,
captcha, threat and spam-filter decisions rely on runtime data and external
configuration, not on the numeric cooldown table alone.

`lib/db.php:40-63` implements MySQL table read-lock/unlock helpers;
`imgboard.php` also uses static/deferred rebuild branches. This identifies
original storage and lock mechanisms, not a complete concurrency proof.
Original external deployment/DB-engine behavior is missing. The rewrite's
PostgreSQL board serialization, handler/Argon2 admission budgets, per-peer
limiting and transport limits remain containment decisions rather than
source-matching cooldowns.

### Deletion

`imgboard.php:2438-2787` authorizes deletion through a supplied password
token, the same peer host, authorized staff/can-delete rights or automatic
maintenance, with board/age/archival/sticky/role restrictions. Public deletion
of certain OPs and OPs with protected staff replies is conditional.
Known-user minimum delay is 60 seconds; the unknown-user branch uses 600.
The configured 1,800-second cutoff applies to the board no-delete rules,
not one universal cutoff for all roles/posts.

`imgboard.php:7541-7555` compares previous action counts using strict
`count(*) > limit`, with declarations 2/hour and 10/day. These are not
equivalent to a rewrite promise of exactly two or ten deletions admitted;
batching and the comparison boundary matter. Staff bypass is conditional.

File-only deletion unlinks original/thumbnail assets and marks
`filedeleted`; it updates OP modification state when applicable.
Post/thread deletion hard-deletes board rows and removes relevant report
rows/aggregate state. Staff deletion of other users' posts may copy selected
content into the deletion log. Logical board-row erasure is therefore not a
promise of complete erasure from logs or backups.

The rewrite's soft deletion, Argon2 credential checks and retained reports
remain explicit exceptions. Learning the old hard-delete branch must not
silently change retention, restore or security policy.

### Archives and retention

`imgboard.php:2819` computes active capacity from the page configuration
(global 10 times 15) and excludes sticky/undead protected threads from the
eligible count. The ordinary expiry branch chooses oldest root activity when
`EXPIRE_NEGLECTED` is set, otherwise post number, then archives or deletes
eligible overflow. Protected threads do not consume this eligible-count cap.
This branch does not reject new OPs merely because all retained threads are
protected. The legacy `LOG_MAX` fallback is a separate branch.

`archive_thread` (`imgboard.php:1746-1885`) sets archived/closed,
clears sticky, host/email/password/Pass fields on OP/replies, preserves
needed generated IDs beforehand and rebuilds the read-only thread.
It removes report/aggregate state below its illegal-report threshold but
retains the thresholded cases. That is more specific than claiming either
all reports always persist or all are always erased.

`trim_archive` (`imgboard.php:2788`) uses configured
`ARCHIVE_MAX_AGE` hours against the archived OP root timestamp, then
hard-deletes the expired thread and assets. Zero disables that age trim.
Global declaration is 276 hours, with board overrides in the public setting tables below.
Static/deferred rebuild scheduling affects when cleanup is materialized;
actual production schedules and backups are missing.

The HTML archive list (`imgboard.php:9253`) selects only the past
three days, orders root descending, limits 3,000 entries and truncates
summaries at 100. Public archive JSON instead lists archived IDs ascending.
The 72-hour HTML window is not the full 276-hour global storage lifetime.
The rewrite's fixed per-thread expiry, bounded archive-count policy, soft
retention and all-pinned rejection remain separate documented behavior.

### Media

`imgboard.php:4806-4815` constructs `tim` from request epoch seconds,
four fractional digits from `microtime`, and a two-digit random 00-99 suffix.
That is not the rewrite's transactional monotonic millisecond counter.

Global thumbnail maxima are 250 by 250 for OPs and 125 by 125 for replies
(`config/global_config.ini:415-419`), with declarations/overrides in the
public setting tables below. The original source emits original-file and JPEG-thumbnail URLs,
and source file-name display truncation is separate from catalog search's
filename field (`imgboard.php:2062`, `catalog.php:105`).

JPEG EXIF removal, PNG chunk cleanup/APNG rejection and GIF cleanup precede
the stored MD5 calculation (`imgboard.php:5200-5259`,
`lib/postfilter.php:295-510`). WebM probing/remuxing has external binary
dependencies (`imgboard.php:8652, 8688`). PDF, WebM audio, spoiler and
size/dimension rules are board/configuration-dependent. An API extension list
does not prove universal posting support.

The original direct multipart/processor/static-file flow is source-known.
It does not specify the rewrite's quarantine capability, one-use approved
attachment, isolated Firecracker worker, generated-RGBA promotion or separate
restricted PNG reader. Those remain deliberate security architecture, not
missing original internals to copy.

Generated PNG bytes/MIME/checksums, PNG at a legacy-looking JPEG route,
unsupported legacy formats, original-download policy and receipt deadlines
are explicit differences. Unknown metadata in existing rewrite manifests
remains unknown; old global values are not evidence of those files' content.
Original external processor versions, binary behavior, deployed media headers,
and missing board-specific asset bytes remain unestablished by this audit.

### Catalog teasers, search and spoilers

`catalog.php:48-186` generates embedded native HTML-catalog data,
not the public API catalog serializer. It records filename plus extension
after UTF-8 conversion, reply/image counts, last-reply data and bump position.
Its comment source is already sanitized/formatted stored HTML.

The original teaser pipeline is now known
(`catalog.php:126-145`, `imgboard.php:272-314`):

- Remove the trailing abbreviated-comment chunk when present.
- Convert runs of HTML breaks to spaces, or newlines on text-only boards.
- On `/b/`, call the 300-length truncation helper with spoiler retention.
- On other boards, replace SJIS spans where enabled and strip tags except `s`.
- Preserve the serialized entity/markup context rather than search raw input.

The truncation helper uses ambient-encoding mbstring length/substrings,
removes incomplete trailing entities/tags, closes open spoilers and adds
U+2026. Its original length/strip order is part of the target, not a generic
300-character truncate everywhere.

`js/catalog.js:1754-1804` composes the teaser as a bold subject,
optionally followed by colon-space and comment, and tests that teaser and the
filename separately. Quick filter uses a 250 ms keyup debounce and native
case-insensitive JavaScript RegExp, with the original escape list
(`js/catalog.js:556-611`). No Unicode RegExp flag is introduced.
Source-known entity/whitespace/link/filename/truncation preparation does not
prove the rewrite's bounded matcher or serializer already matches.

The filename is assigned before the deleted-image presentation branch and is
not removed there (`catalog.php:105, 177`). Deleted filenames excluded
from rewrite search are therefore a local privacy/representation difference.
Missing-field coercion behavior must also be kept distinct from deliberately
bounded rewrite inputs.

The source client handles catalog display preferences, session search,
pin/hide maps and watcher settings using native storage keys:
`catalog-settings`, `catalog-theme`, `4chan-catalog-search`,
`4chan-pin-<board>`, `4chan-hide-t-<board>` and watcher keys.
The declaration tables and supplied scripts establish original defaults; implementing
all hover/theme/navigation options still remains work.

HTML catalog spoiler suffix comes from the configured board count
(`catalog.php:37`, `js/catalog.js:1841-1845`). The extension
reuses an existing page's custom suffix when available, otherwise randomly
selects 1 through the supplied count and caches it per board
(`js/extension.js:410-421`). These selection rules and declared board
counts are known; missing board-specific image bytes are not invented.
Original file/deleted/no-file and spoiler-preference branches are explicit
(`js/catalog.js:1893-1924`), not one universal fallback image rule.

### Layout and themes

`views/imgboard.php` and `imgboard.php:1915` establish post/form
DOM structure, file/header/comment ordering, menus/navigation anchors,
archive states and desktop/mobile conditional blocks. The supplied six desktop
and two mobile theme stylesheets expose selectors, geometry, colors and
responsive rules; `css/catalog_mobile.css` adds catalog/mobile rules.
Server error/success/archive branches are also available as static source.

Catalog style grouping explicitly reads the work-safe/non-work-safe cookie,
defaulting to Yotsuba B New versus Yotsuba New
(`js/catalog.js:1441-1454`). Finite host-only HttpOnly preferences,
same-origin forms and fixed local assets in the rewrite remain E-009.

Source CSS/templates resolve broad unknown structure and selector questions;
they do not supply original full rendered-page screenshots. Browser defaults,
fonts, dynamic content, external widgets/adverts, asset versions and actual
state transitions still require a controlled original/reference comparison.
Synthetic rewrite screenshots remain regressions, not 1:1 proof.

### Native extension

`js/extension.js` supplies the original native mechanisms; its declared
version is distinct from the separately pinned public v1191.

Watcher (`5214-5870`) stores `4chan-watch` and
`4chan-watch-bl`, constructs labels from subject or break/tag-stripped
comment with 45-unit JavaScript slicing, and uses an ID fallback.
Read marker 0 is distinct from dead marker -1; row fragments include
`#lr`, unread counts and tracked-reply/archive/dead presentation.
Successful refresh counts replies newer than the marker, detects tracked
quotes and sets archive state; 404 records dead state. Auto-refresh is
throttled at 60 seconds and per-thread fetches are staggered by 200 ms.
Catalog auto-watching and its blacklist are explicit original branches.

Thread hiding (`4832`) uses `4chan-hide-t-<board>` and
`4chan-purge-t-<board>`; its twelve-hour interval controls successful
live-thread-list pruning, not individual hidden-thread expiry.
Ordinary reply hiding (`5013`) has seven-day expiry and separate recursive
helper/storage definitions. Rewrite record bounds, Web Locks and conflict
checks remain containment differences.

Post menu (`1616-1729`) conditionally adds report, OP hide/watch,
ordinary reply hide, mobile deletion, image search/open and selection-filter
actions. It dispatches `4chanPostMenuReady` synchronously with post ID,
OP state and the live detached menu before appending it. The built-in menu
does not expose recursive reply hiding merely because `toggleR` exists.
External menu-ready subscribers are a separate unknown integration input.

Updater (`6089-6720`) provides these source-known rules:

- Delay ladder: 10, 15, 20, 30, 60, 90, 120, 180, 240, 300 seconds; hidden-tab
  minimum ladder index is 4.
- Separate full/tail Last-Modified values and conditional-date requests;
  reply-window/age checks select tail versus full.
- Missing tail boundary or tail 404 retries full; full 404 terminates;
  304 and status 0 follow the no-new-post path.
- Append newer post IDs, parse/filter/track them and update flags/statistics;
  existing-post deletion reconciliation is not implemented in the old updater.
  Its deletionQueue appears only at initialization.
- Quick Reply sets a single lastReplyId; an update containing exactly that one
  own reply suppresses ordinary unread/new-reply notification and clears it.
  Ordinary posting receipts are not a substitute for this lifecycle.
- Automatic versus forced updates have different unread-marker/icon behavior.
  Hidden-tab/bottom-position checks gate automatic scrolling.
- Tracked-quote notification outranks filter-highlight and ordinary-new-post
  icons; terminal thread state selects the dead icon.
- Optional sound uses beep.ogg for a hidden automatic tracked-reply notification
  when the page sound control is enabled. The setting defaults to false.
- New-post insertion dispatches 4chanThreadUpdated with its count. Tail,
  validators, marker edges and watcher acknowledgements are concrete targets,
  not unspecified native behavior.

The owned full/tail renderer projections, exact string identifiers, DOM budgets
and one-second update-cycle floor are deliberate substitutions. The
[tail implementation](native-updater-tail.md) follows the source selection and
fallback rules, with separate validators and bounded transport. Fetch uses
manual conditional headers; response bodies are revalidatable.
Deletion reconciliation, if added locally, should be labeled an enhancement,
not a missing original updater feature.

Config/defaults (`8795-8848`) and Settings (`8968`) expose more than
the implemented watcher subset, including Quotes, Monitoring, Filters/Post
Hiding, Navigation, Images/Media and Miscellaneous categories. The native default table below
records native boolean defaults and mobile overrides. Quick Reply, quote
preview/backlinks, updater and thread hiding default true; watcher, shortcuts,
sound and many optional features default false. Full settings and feature
matching remain incomplete.

Keybinds (`8237`) confirm A/F/Q/R/W/B/C/N/I with runtime guards and
editable-target/modifier exclusions. Original B/N submit pagination forms;
bounded GET-link navigation is a rewrite difference. Q now opens the
[Quick Reply dialog](native-quick-reply.md); its remaining shortcut group and
complete source lifecycle are unfinished.

### Reports and staff

`modes/report.php:307, 517-666` loads category definitions/scopes from
database rows, validates post/category and stores a post snapshot, reporter
identity and category weight. Unknown/threat/filtered conditions can reduce
weight. Duplicate insertion and aggregate counters use explicit SQL branches.
Previously cleared reports can cause new reports of that post to inherit
cleared state. Hour/day checks compare against configured 30/80 thresholds
(`modes/report.php:157-167`).

`forms/report.php` supplies the original popup/rule/category/illegal-report
UI. `admin.php:139, 1552-1588` supplies protected queue/clearing behavior;
clearing marks reports cleared rather than constituting a universal content
erasure. Deletion/archival report cleanup is described separately above.
Actual category labels/weights and other database seed rows are not supplied
merely because the category selector/queries exist.

`lib/auth.php:89, 130-154, 221` establishes staff cookie validation,
hierarchical levels, board-scoped flags and OTP hooks using external salts and
account configuration. The active cookie path derives the expected SHA-256
token from account/password material and external salt; the commented
password_verify alternative is not the active request-authentication branch.
The public Pass login and email sign-in sources are different systems.

Original staff/password/role/queue mechanisms are therefore source-known.
Credentials, external secrets, account/ban/category state, deployed enrollment,
hardware, incident/recovery and independent-review policy remain missing.
The rewrite's separate-origin WebAuthn app, required user verification,
operator invitations/recovery, recent-authentication/CSRF checks and all-board
role model remain deliberate replacements, not claims of copied old workflow.

### HTTP and deployment

Static JSON generation is not evidence of the deployed API server's ETags,
CORS, exposed headers, compression, error bodies, CDN behavior or DNS mapping.
Source request-method/referrer checks and individual redirect branches can be
described without claiming that whole deployment contract is known.
`imgboard.php:6883-6929` resolves old post redirects with a 301 and short
public cache; HTML posting success uses meta-refresh. The rewrite's 303/308
and JSON-only CORS listener remain stated local behavior.

Missing external config paths, absent subdomain configuration and the required
but unavailable `lib/twister_captcha.php`, captcha fonts/keys/services,
database schema/seed/runtime state, processor binaries and actual proxy/server
configuration prevent a complete runnable reconstruction. Do not execute
the old source or weaken rewrite containment to guess these inputs.

### Compatibility coverage

Each ID has source detail above and a concrete remaining distinction.

| ID | Source-known original target / remaining distinction |
|---|---|
| I-001 | Loaded board metadata and public setting declarations; missing external/default/runtime inputs are explicit. |
| I-002 | Conditional thread fields, counts, identities and tail; omitted rewrite identities remain feature gaps. |
| I-003 | Thread-list page groups and visible replies, not lifetime counts. |
| I-004 | Index OP/last-five replies and omission counts. |
| I-005 | Public API catalog serialization is distinct from native HTML-catalog data. |
| I-006 | Ascending archive IDs, source rollover/redaction/expiry; rewrite bounded/fixed policy differs. |
| I-007 | Original client's conditional-date use is known; complete CDN/server validator policy is not. |
| I-008 | Templates, DOM/menu/navigation and CSS rules are available; complete matching/rendering remains unqualified. |
| I-009 | Multipart text fields/modes and exact Accept JSON responses are implemented; original identity, single-request file posting and complete client flow remain unfinished. |
| I-010 | Source-specific methods/301/meta-refresh are known; proxy/CORS/status/header deployment remains missing. |
| B-001 | Original posting/identity/transformation flow is known; PostgreSQL persistence is a replacement. |
| B-002 | Sage substring processing and current-count bump decisions differ from exact options/lifetime rules. |
| B-003 | Board/category limits, image admission and protected-thread counting are known; local bounds differ. |
| B-004 | Token/host/staff/automatic deletion branches are known; Argon2/soft deletion differ. |
| B-005 | Active escaping/markup/link/quote pipeline is known; bounded grammar does not imply matching. |
| B-006 | Popup categories, weighted queue and clear inheritance are known; database rows/policy are missing. |
| B-007 | Cookie/role/OTP mechanisms are known; WebAuthn and current production qualification remain separate. |
| B-008 | Original option parsing, tid/pid JSON and meta-refresh are known; 303/allowlist differ. |
| M-001 | Board-dependent intake/cleanup/formats are known; PNG/JPEG-only development and production disablement remain. |
| M-002 | Original direct upload path is known; quarantine/queue are containment architecture. |
| M-003 | Original processing/hash order is known; normalized RGBA/PNG promotion is a replacement. |
| M-004 | External original processing dependencies are identified; isolated execution is a rewrite requirement. |
| M-005 | Original file/thumbnail shapes are known; deployed media headers and remaining formats are separate. |
| M-006 | Original identity/captcha/direct multipart path is known; capability intake is a replacement. |
| M-007 | tim, thumbnail declarations and deletion effects are known; PNG/MIME/checksum/receipt policy differs. |
| M-008 | Blank image replies versus subject/comment-required ordinary OPs are now distinguished. |
| V-001 | Desktop/mobile templates/selectors are known; full rendered geometry remains unverified. |
| V-002 | Static state/template/CSS branches are known; original state screenshots/loading behavior remain missing. |
| V-003 | Theme grouping/defaults/selectors are known; cookies/CSP and rendered matching remain distinct. |
| V-004 | Original hidden/toggle/noscript form and controls are known; expanded isolated flow differs. |
| V-005 | Board-dependent original teaser preparation is known; shared rewrite fields still need matching. |
| V-006 | Native controls/defaults/storage/search are known; bounded GET fallback/layout differ. |
| V-007 | Image-state precedence and custom-spoiler selection are known; board-specific bytes/matching remain. |
| V-008 | SQL visible counts and branch-specific exclusions are known; lifetime bump count differs. |
| V-009 | Subject/teaser/file serialization, search/storage/pin/hide are known; #82 is matching work. |
| V-010 | Original spoiler preference/suffix behavior is known; board assets and fallback extensions remain. |
| V-011 | Watcher/menu/hiding/events/keys/updater/notifications/QR rules are known; unfinished features and live qualification remain. |
| E-001 | Original supported formats are conditional; normalized PNG-only publication remains deliberate. |
| E-002 | Ordinary escaping versus privileged purifier branch is explicit; all public typed escaping remains. |
| E-003 | Original staff/public identity mechanisms are explicit; separate WebAuthn/Argon2 remain. |
| E-004 | Original visible-host unique count is known; omission remains explicit. |
| E-005 | Newline-then-ambient-mb_strlen order is known; ambient deployment encoding and local differences remain. |
| E-006 | Cooldown/delete/action/lock/rollover branches are known; distributed deployment and local budgets remain separate. |
| E-007 | Hard deletion, selected logs, report cleanup and archive redaction/expiry are known; backups/policy remain missing. |
| E-008 | Original cleanup-before-MD5/JPEG-thumb behavior is known; generated PNG bytes/MIME/hashes differ. |
| E-009 | Original cookie/style grouping is known; host-only HttpOnly preferences/forms remain. |
| E-010 | Single multipart and JSON/meta-refresh sequencing are known; isolated approval/post/303 differs. |
| E-011 | Original toggle/noscript/file/captcha structure is known; expanded form/security/mobile adaptations remain. |

### Remaining unknowns

These are missing evidence, not broad claims that the original algorithm is
unspecified:

- Whether modern assets or deployed behavior differ from this supplied old
  snapshot; old declarations and the newer public pins are not one identical release.
- Production mbstring encoding and externally loaded configuration, absent
  subdomain defaults, dynamic/random configuration and database board metadata.
- Database schema/seed and runtime category/flag/account/ban/filter/identity
  data, external salts/keys and geolocation/model/service inputs.
- Missing Twister implementation, captcha font/service behavior and live
  abuse/admission outcomes; source UI alone is not captcha-service equivalence.
- External media binary versions/behavior, missing board-specific image bytes
  and deployed original-file/media MIME/cache policies.
- Original full rendered desktop/mobile/state/loading reference, browser/fonts
  and external UI integrations, including third-party menu-ready subscribers.
- API/CDN/proxy headers/status/cache/CORS/domain mapping, effective rebuild/
  maintenance schedules, backup retention and operational erasure/recovery.

Source-known implementation gaps remain separately tracked: full formatting/
search-field matching, custom-board assets, Quick Reply and related lifecycle,
remaining native settings/features, omitted API identity fields, unsupported media formats and full rendered-page comparison.
Deliberate security/retention exceptions require an explicit decision, not an
invented parity claim. The original source audit changed documentation only and ran no tests. Later
implementation checks are recorded in their linked feature contracts.

### Public setting declarations

These are whitelisted public declarations, not flattened effective production settings.
Values are quoted as stored, with source line numbers. Global, category and board
layers must be combined using the loader's rules; missing/external values are not guessed.
All paths below are relative to `4chan-old/`. Board files without CATEGORY require
the missing subdomain-group configuration rather than an invented ws/nws default.

| Configuration file | Public declarations (source line) |
|---|---|
| `config/boards/3.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/a.config.ini` | `ARCHIVE_MAX_AGE=180` (L15); `NO_DELETE_OP=yes` (L18); `SPOILERS=yes` (L21); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-a1.png` (L23); `SPOILER_NUM=1` (L25); `CATEGORY=ws` (L32); `MAX_USER_THREADS=3` (L40); `MAX_RES=500` (L44); `MAX_IMGRES=300` (L45) |
| `config/boards/aco.config.ini` | `CATEGORY=nws` (L27) |
| `config/boards/adv.config.ini` | `CATEGORY=ws` (L16) |
| `config/boards/an.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/asp.config.ini` | `CATEGORY=ws` (L16) |
| `config/boards/b.config.ini` | `RENZOKU=30` (L15); `RENZOKU2=30` (L17); `RENZOKU_INTRA=30` (L19); `RENZOKU3=90` (L23); `MAX_KB=2048` (L28); `MAX_WEBM_FILESIZE=2048` (L29); `REPLIES_SHOWN=3` (L31); `MAX_LINES_SHOWN=10` (L33); `MAX_LINES=50` (L35); `MAX_RES=300` (L39); `MAX_IMGRES=150` (L41); `RENZOKU_OP=yes` (L45); `RENZOKU_OP_TIME=600` (L47); `EXPIRE_NEGLECTED=yes` (L50); `ENABLE_ARCHIVE=no` (L55); `FORCED_ANON=no` (L73); `DISP_ID=no` (L79); `CATEGORY=nws` (L94) |
| `config/boards/bant.config.ini` | `CATEGORY=nws` (L18); `NO_DELETE_OP=yes` (L21); `DISP_ID=yes` (L24); `SHOW_COUNTRY_FLAGS=yes` (L30); `ENABLE_ARCHIVE=no` (L33); `RENZOKU=15` (L41); `RENZOKU2=15` (L43); `RENZOKU_INTRA=15` (L45); `RENZOKU3=60` (L49); `MAX_KB=2048` (L54); `MAX_WEBM_FILESIZE=2048` (L55); `REPLIES_SHOWN=3` (L57); `MAX_LINES_SHOWN=10` (L59); `MAX_LINES=50` (L61); `MAX_RES=300` (L65); `MAX_IMGRES=150` (L67); `RENZOKU_OP=yes` (L71); `RENZOKU_OP_TIME=600` (L73); `MAX_USER_THREADS=3` (L75); `EXPIRE_NEGLECTED=yes` (L78) |
| `config/boards/biz.config.ini` | `CATEGORY=ws` (L17); `DISP_ID=yes` (L20); `DEF_PAGES=20` (L26) |
| `config/boards/c.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/cgl.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/ck.config.ini` | `CATEGORY=ws` (L16) |
| `config/boards/cm.config.ini` | `CATEGORY=ws` (L18) |
| `config/boards/co.config.ini` | `SPOILERS=yes` (L18); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-co1.png,spoiler-co2.png,spoiler-co3.png,spoiler-co4.png,spoiler-co5.png}}` (L20); `SPOILER_NUM=5` (L22); `CATEGORY=ws` (L29); `MAX_RES=500` (L33); `MAX_IMGRES=300` (L34) |
| `config/boards/d.config.ini` | `CATEGORY=nws` (L23) |
| `config/boards/diy.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/e.config.ini` | `CATEGORY=nws` (L23) |
| `config/boards/f.config.ini` | `NO_TEXTONLY=yes` (L6); `ENABLE_ARCHIVE=no` (L9); `CAPTCHA_TWISTER=no` (L14); `LOG_MAX=500` (L19); `DEF_PAGES=30` (L21); `PAGE_MAX=1` (L22); `EXPIRE_NEGLECTED=no` (L23); `MAX_KB=10240` (L26); `CATEGORY=nws` (L45) |
| `config/boards/fa.config.ini` | `CATEGORY=ws` (L18) |
| `config/boards/fit.config.ini` | `CATEGORY=ws` (L18) |
| `config/boards/g.config.ini` | `CATEGORY=ws` (L20); `CODE_TAGS=yes` (L23) |
| `config/boards/gd.config.ini` | `CATEGORY=ws` (L14); `MAX_KB=8192` (L19); `MAX_DIMENSION=10000` (L21) |
| `config/boards/gif.config.ini` | `MAX_KB=4096` (L12); `MAX_WEBM_FILESIZE=4096` (L14); `MAX_WEBM_DURATION=300` (L16); `ENABLE_WEBM_AUDIO=yes` (L18); `CATEGORY=nws` (L29); `ARCHIVE_MAX_AGE=24` (L39); `PAGE_MAX=5` (L42) |
| `config/boards/h.config.ini` | `CATEGORY=nws` (L21) |
| `config/boards/hc.config.ini` | `CATEGORY=nws` (L16); `MAX_KB=8192` (L21); `MAX_DIMENSION=8000` (L23); `MIN_W=500` (L26); `MIN_H=500` (L27) |
| `config/boards/his.config.ini` | `PERMASAGE_HOURS=168` (L8); `CATEGORY=ws` (L19); `NO_DELETE_OP=yes` (L22) |
| `config/boards/hm.config.ini` | `MAX_IMGRES=150` (L15); `MAX_KB=8192` (L17); `MAX_DIMENSION=8000` (L19); `CATEGORY=nws` (L28) |
| `config/boards/hr.config.ini` | `MAX_KB=8192` (L10); `MIN_W=1000` (L12); `MIN_H=1000` (L13); `MAX_DIMENSION=10000` (L15); `CATEGORY=nws` (L24) |
| `config/boards/i.config.ini` | `MAX_USER_THREADS=3` (L23); `MAX_USER_THREADS_PERIOD=168` (L25); `CATEGORY=nws` (L33) |
| `config/boards/ic.config.ini` | `CATEGORY=nws` (L17) |
| `config/boards/int.config.ini` | `CATEGORY=ws` (L16); `NO_DELETE_OP=yes` (L19); `SHOW_COUNTRY_FLAGS=yes` (L22); `ARCHIVE_MAX_AGE=235` (L25) |
| `config/boards/j.config.ini` | `ENABLE_ARCHIVE=no` (L11); `SHOW_THREAD_UNIQUES=no` (L13); `DISP_ID=no` (L14); `LOG_MAX=1000000000` (L27); `PAGE_MAX=0` (L29); `MAX_RES=1000` (L32); `CATEGORY=nws` (L47); `NO_TEXTONLY=no` (L68); `FAVICON=//s.4cdn.org/image/favicon-j.ico` (L71); `CAPTCHA=no` (L80); `CODE_TAGS=yes` (L86); `MAX_COM_CHARS_AUTHED=50000` (L87) |
| `config/boards/jp.config.ini` | `AUTOARCHIVE_CAP=1500` (L12); `CATEGORY=ws` (L23); `NO_DELETE_OP=yes` (L26); `RENZOKU3=3600` (L29); `MAX_IMGRES=300` (L32); `SPOILERS=yes` (L35); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-jp1.png` (L37); `SPOILER_NUM=1` (L39); `SJIS_TAGS=yes` (L41); `MAX_COM_CHARS=5000` (L43); `ARCHIVE_MAX_AGE=250` (L49) |
| `config/boards/k.config.ini` | `CATEGORY=ws` (L19) |
| `config/boards/lgbt.config.ini` | `CATEGORY=ws` (L19); `ENABLE_BOARD_FLAGS=no` (L25) |
| `config/boards/lit.config.ini` | `CATEGORY=ws` (L14); `SPOILERS=yes` (L17); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-lit1.png` (L19); `SPOILER_NUM=1` (L21); `MAX_COM_CHARS=3000` (L26) |
| `config/boards/m.config.ini` | `SPOILERS=yes` (L8); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-m1.png,spoiler-m2.png,spoiler-m3.png,spoiler-m4.png}}` (L10); `SPOILER_NUM=4` (L12); `CATEGORY=ws` (L19) |
| `config/boards/mlp.config.ini` | `CATEGORY=ws` (L18); `SPOILERS=yes` (L21); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-mlp1.png` (L23); `SPOILER_NUM=1` (L25); `MAX_COM_CHARS=3000` (L30); `ENABLE_BOARD_FLAGS=yes` (L33); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L40) |
| `config/boards/mu.config.ini` | `CATEGORY=ws` (L16) |
| `config/boards/n.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/news.config.ini` | `TEXT_ONLY=yes` (L11); `PERMASAGE_HOURS=48` (L13); `SPOILERS=yes` (L20); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-a1.png` (L22); `SPOILER_NUM=1` (L24); `CATEGORY=ws` (L31); `MAX_USER_THREADS=5` (L33); `MAX_USER_THREADS_PERIOD=120` (L34); `MAX_RES=500` (L42); `MAX_IMGRES=300` (L43) |
| `config/boards/o.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/out.config.ini` | `CATEGORY=ws` (L18); `MAX_KB=5120` (L23); `MAX_DIMENSION=8000` (L25) |
| `config/boards/p.config.ini` | `MAX_KB=5120` (L10); `STRIP_EXIF=no` (L16); `CATEGORY=ws` (L26) |
| `config/boards/po.config.ini` | `MAX_KB=8192` (L10); `CATEGORY=ws` (L20) |
| `config/boards/pol.config.ini` | `MAX_USER_THREADS=3` (L63); `MAX_USER_THREADS_PERIOD=6` (L65); `ARCHIVE_MAX_AGE=72` (L72); `CATEGORY=nws` (L79); `NO_DELETE_OP=yes` (L82); `DISP_ID=yes` (L85); `SHOW_COUNTRY_FLAGS=yes` (L91); `ENABLE_BOARD_FLAGS=yes` (L97); `RENZOKU=30` (L108); `RENZOKU2=30` (L110); `RENZOKU_INTRA=30` (L112); `RENZOKU3=90` (L116); `DEF_PAGES=20` (L121) |
| `config/boards/pw.config.ini` | `CATEGORY=ws` (L14); `NO_DELETE_OP=yes` (L17) |
| `config/boards/qa.config.ini` | `CATEGORY=ws` (L19); `NO_DELETE_OP=yes` (L22); `MAX_USER_THREADS_PERIOD=48` (L24); `MAX_USER_THREADS=3` (L25); `PERMASAGE_HOURS=168` (L28) |
| `config/boards/qb.config.ini` | `CATEGORY=nws` (L16) |
| `config/boards/qst.config.ini` | `CATEGORY=ws` (L16); `SPOILERS=yes` (L19); `REQUIRE_SUBJECT=yes` (L32); `OP_MARKUP=yes` (L34); `MAX_USER_THREADS=5` (L37); `MAX_USER_THREADS_PERIOD=72` (L39); `MAX_COM_CHARS=3000` (L43); `NO_DELETE_OP=yes` (L45); `DISP_ID=yes` (L48); `PERMASAGE_HOURS=120` (L55); `MAX_RES=750` (L58); `MAX_IMGRES=375` (L60); `MAX_LINES=100` (L66); `MAX_KB=8192` (L72) |
| `config/boards/r.config.ini` | `CATEGORY=nws` (L17); `MAX_KB=8192` (L22); `MAX_DIMENSION=10000` (L24); `ENABLE_WEBM_AUDIO=yes` (L26) |
| `config/boards/r9k.config.ini` | `MAX_RES=500` (L16); `MAX_IMGRES=150` (L18); `RENZOKU3=600` (L20); `EXPIRE_NEGLECTED=yes` (L23); `SPOILERS=yes` (L28); `CATEGORY=nws` (L35) |
| `config/boards/s.config.ini` | `CATEGORY=nws` (L16); `MAX_IMGRES=150` (L20); `MAX_KB=8192` (L23); `MAX_DIMENSION=8000` (L25); `MIN_W=500` (L28); `MIN_H=500` (L29) |
| `config/boards/s4s.config.ini` | `MAX_KB=2048` (L18); `RENZOKU3=300` (L32); `RENZOKU_OP=yes` (L34); `RENZOKU_OP_TIME=120` (L36); `CATEGORY=nws` (L45); `SPOILERS=yes` (L63); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-s4s1.png,spoiler-s4s2.png,spoiler-s4s3.png,spoiler-s4s4.png,spoiler-s4s5.png,spoiler-s4s6.png}}` (L65); `SPOILER_NUM=6` (L67); `FORCED_ANON=no` (L73); `DISP_ID=no` (L76) |
| `config/boards/sci.config.ini` | `CATEGORY=ws` (L14); `JSMATH=yes` (L17); `MAX_IMGRES=250` (L23); `MAX_LINES_SHOWN=20` (L26) |
| `config/boards/soc.config.ini` | `CATEGORY=nws` (L16); `STRIP_EXIF=yes` (L20); `DISP_ID=yes` (L29); `MAX_RES=500` (L36); `MAX_IMGRES=300` (L38); `RENZOKU3=600` (L40); `RENZOKU_OP=yes` (L42); `RENZOKU_OP_TIME=300` (L44); `MAX_KB=5120` (L46); `MAX_DIMENSION=8000` (L48); `EXPIRE_NEGLECTED=yes` (L51) |
| `config/boards/sp.config.ini` | `ARCHIVE_MAX_AGE=250` (L12); `CATEGORY=ws` (L19); `NO_DELETE_OP=yes` (L26); `SHOW_COUNTRY_FLAGS=yes` (L29); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L40) |
| `config/boards/t.config.ini` | `CATEGORY=nws` (L14); `REPLIES_SHOWN=1` (L19); `MAX_LINES_SHOWN=6` (L21) |
| `config/boards/test.config.ini` | `MAX_IMG_REPOST_COUNT=0` (L18); `REPLIES_SHOWN=5` (L22); `OP_MARKUP=yes` (L52); `MAX_USER_THREADS=50` (L55); `MAX_KB=4096` (L71); `MAX_WEBM_FILESIZE=4096` (L73); `ENABLE_WEBM_AUDIO=yes` (L77); `ENABLE_ARCHIVE=yes` (L94); `LOG_MAX=500` (L106); `CODE_TAGS=yes` (L127); `SJIS_TAGS=no` (L128); `STRIP_EXIF=yes` (L130); `CATEGORY=ws` (L152); `SPOILERS=yes` (L174); `FAVICON=//s.4cdn.org/image/favicon-test.ico` (L178); `RENZOKU3=30` (L201) |
| `config/boards/tg.config.ini` | `CATEGORY=ws` (L18); `SPOILERS=yes` (L21); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-tg1.png,spoiler-tg2.png}}` (L23); `SPOILER_NUM=2` (L25); `MAX_KB=8192` (L39) |
| `config/boards/toy.config.ini` | `PERMASAGE_HOURS=336` (L11); `CATEGORY=ws` (L18) |
| `config/boards/trash.config.ini` | `CATEGORY=nws` (L14); `EXPIRE_NEGLECTED=yes` (L17); `ENABLE_ARCHIVE=no` (L20); `DISP_ID=no` (L32) |
| `config/boards/trv.config.ini` | `MAX_KB=8192` (L10); `MAX_DIMENSION=10000` (L12); `CATEGORY=ws` (L21) |
| `config/boards/tv.config.ini` | `ARCHIVE_MAX_AGE=250` (L14); `CATEGORY=ws` (L21); `NO_DELETE_OP=yes` (L24); `SPOILERS=yes` (L27); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-tv1.png,spoiler-tv2.png,spoiler-tv3.png,spoiler-tv4.png,spoiler-tv5.png}}` (L29); `SPOILER_NUM=5` (L31) |
| `config/boards/u.config.ini` | `CATEGORY=nws` (L21); `EXPIRE_NEGLECTED=yes` (L24); `SPOILERS=yes` (L27) |
| `config/boards/v.config.ini` | `DEF_PAGES=20` (L8); `ARCHIVE_MAX_AGE=170` (L17); `NO_DELETE_OP=yes` (L28); `SPOILERS=yes` (L34); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-v1.png` (L36); `SPOILER_NUM=1` (L38); `CATEGORY=ws` (L45); `MAX_USER_THREADS=3` (L51); `RENZOKU_OP_TIME=600` (L54); `MAX_RES=500` (L58); `MAX_IMGRES=300` (L59) |
| `config/boards/vg.config.ini` | `REQUIRE_SUBJECT=yes` (L21); `ARCHIVE_MAX_AGE=140` (L24); `SPOILERS=yes` (L27); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-vg1.png` (L29); `SPOILER_NUM=1` (L31); `CATEGORY=ws` (L38); `MAX_W=200` (L43); `MAX_H=200` (L44); `MAX_RES=750` (L49); `MAX_IMGRES=375` (L51); `DEF_PAGES=20` (L55); `MAX_LINES=100` (L57); `REPLIES_SHOWN=0` (L59); `RENZOKU=90` (L62); `RENZOKU2=120` (L64) |
| `config/boards/vip.config.ini` | `SHOW_THREAD_UNIQUES=no` (L11); `NO_DELETE_OP=yes` (L18); `SPOILERS=yes` (L21); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler.png` (L23); `CATEGORY=ws` (L32); `SJIS_TAGS=yes` (L36); `MAX_RES=300` (L40); `MAX_IMGRES=300` (L41) |
| `config/boards/vm.config.ini` | `ARCHIVE_MAX_AGE=120` (L12); `NO_DELETE_OP=yes` (L17); `SPOILERS=yes` (L20); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-v1.png` (L22); `SPOILER_NUM=1` (L24); `CATEGORY=ws` (L31); `MAX_USER_THREADS=3` (L34); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L39) |
| `config/boards/vmg.config.ini` | `ARCHIVE_MAX_AGE=120` (L12); `NO_DELETE_OP=yes` (L17); `SPOILERS=yes` (L20); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-vmg1.png,spoiler-vmg2.png,spoiler-vmg3.png}}` (L22); `SPOILER_NUM=3` (L24); `CATEGORY=ws` (L31); `MAX_USER_THREADS=3` (L34); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L39) |
| `config/boards/vp.config.ini` | `CATEGORY=ws` (L16); `SPOILERS=yes` (L22); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-vp1.png` (L24); `SPOILER_NUM=1` (L26) |
| `config/boards/vr.config.ini` | `PERMASAGE_HOURS=336` (L8); `CATEGORY=ws` (L17); `SPOILERS=yes` (L23); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-vr1.png,spoiler-vr2.png}}` (L25); `SPOILER_NUM=2` (L27); `MAX_RES=500` (L31); `MAX_IMGRES=300` (L32) |
| `config/boards/vrpg.config.ini` | `ARCHIVE_MAX_AGE=160` (L12); `NO_DELETE_OP=yes` (L17); `SPOILERS=yes` (L20); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-vrpg1.png,spoiler-vrpg2.png,spoiler-vrpg3.png}}` (L22); `SPOILER_NUM=3` (L24); `CATEGORY=ws` (L31); `MAX_USER_THREADS=3` (L34); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L39) |
| `config/boards/vst.config.ini` | `ARCHIVE_MAX_AGE=120` (L12); `NO_DELETE_OP=yes` (L17); `SPOILERS=yes` (L20); `SPOILER_THUMB={{STATIC_SERVER}}image/spoiler-vst.png` (L22); `SPOILER_NUM=1` (L24); `CATEGORY=ws` (L31); `MAX_USER_THREADS=3` (L34); `MAX_RES=500` (L38); `MAX_IMGRES=300` (L39) |
| `config/boards/vt.config.ini` | `AUTOARCHIVE_CAP=1500` (L7); `CATEGORY=ws` (L20); `NO_DELETE_OP=yes` (L23); `RENZOKU3=3600` (L26); `MAX_IMGRES=300` (L29); `SPOILERS=yes` (L32); `SPOILER_THUMB={{STATIC_SERVER}}image/{{!rand spoiler-vt1.png,spoiler-vt2.png,spoiler-vt3.png}}` (L34); `SPOILER_NUM=3` (L36); `MAX_COM_CHARS=5000` (L38); `ARCHIVE_MAX_AGE=72` (L43) |
| `config/boards/w.config.ini` | `MAX_KB=6144` (L10); `MIN_W=480` (L12); `MIN_H=600` (L13); `CATEGORY=ws` (L22); `MAX_IMGRES=300` (L25) |
| `config/boards/wg.config.ini` | `MAX_KB=6144` (L13); `MIN_W=480` (L15); `MIN_H=600` (L16); `CATEGORY=nws` (L25) |
| `config/boards/wsg.config.ini` | `MAX_IMG_REPOST_COUNT=3` (L8); `MAX_KB=6144` (L13); `MAX_WEBM_FILESIZE=6144` (L15); `MAX_WEBM_DURATION=400` (L17); `CATEGORY=ws` (L26); `ENABLE_WEBM_AUDIO=yes` (L34); `ARCHIVE_MAX_AGE=48` (L39) |
| `config/boards/wsr.config.ini` | `CATEGORY=ws` (L14); `MAX_KB=8192` (L19); `MAX_DIMENSION=10000` (L21); `ENABLE_WEBM_AUDIO=yes` (L23) |
| `config/boards/x.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/xs.config.ini` | `CATEGORY=ws` (L14) |
| `config/boards/y.config.ini` | `CATEGORY=nws` (L20); `EXPIRE_NEGLECTED=yes` (L23) |
| `config/categories/nws.config.ini` | `FAVICON={{STATIC_SERVER}}image/favicon.ico` (L6); `RENZOKU=60` (L11); `RENZOKU2=30` (L13); `RENZOKU_INTRA=60` (L15); `RENZOKU3=600` (L19); `MAX_RES=300` (L22); `MAX_IMGRES=300` (L24) |
| `config/categories/ws.config.ini` | `FAVICON={{STATIC_SERVER}}image/favicon-ws.ico` (L11); `DEFAULT_BURICHAN=yes` (L14); `RENZOKU=60` (L24); `RENZOKU2=60` (L26); `RENZOKU_INTRA=60` (L28); `RENZOKU3=600` (L32); `MAX_RES=310` (L35); `MAX_IMGRES=150` (L37); `MAX_LINES=100` (L40) |
| `config/global_config.ini` | `JS_VERSION_CORE=1123` (L6); `JS_VERSION_EXT=1178` (L7); `JS_VERSION_CATALOG=1024` (L8); `CSS_VERSION=715` (L12); `CSS_VERSION_CATALOG=705` (L13); `CSS_VERSION_FLAGS=690` (L14); `ENABLE_ARCHIVE=yes` (L36); `ARCHIVE_MAX_AGE=276` (L38); `CAPTCHA=yes` (L43); `CAPTCHA_TWISTER=yes` (L44); `STICKY_CAP=1000` (L50); `AUTOARCHIVE_CAP=0` (L52); `CODE_TAGS=no` (L81); `SJIS_TAGS=no` (L82); `TEXT_ONLY=no` (L126); `TEXT_ONLY_ALLOW_OP=no` (L128); `PERMASAGE_HOURS=0` (L130); `LOG_MAX=700` (L147); `PAGE_MAX=10` (L149); `DEF_PAGES=15` (L151); `MAX_RES=300` (L153); `REPLIES_SHOWN=5` (L155); `MAX_LINES_SHOWN=15` (L157); `MAX_LINES=70` (L159); `MAX_COM_CHARS=2000` (L162); `MAX_COM_CHARS_AUTHED=10000` (L164); `MAX_IMGRES=150` (L167); `MAX_KB=4096` (L169); `MIN_W=1` (L171); `MIN_H=1` (L172); `MAX_DIMENSION=10000` (L174); `ENABLE_WEBM=yes` (L182); `ENABLE_WEBM_AUDIO=no` (L184); `MAX_WEBM_DURATION=120` (L188); `MAX_WEBM_FILESIZE=4096` (L190); `MAX_USER_THREADS=5` (L193); `MAX_USER_THREADS_PERIOD=24` (L195); `MAX_IMG_REPOST_COUNT=0` (L198); `RENZOKU=60` (L201); `RENZOKU2=60` (L203); `RENZOKU_INTRA=60` (L205); `RENZOKU3=600` (L209); `RENZOKU_DUPE=300` (L213); `RENZOKU_SAGE=120` (L215); `RENZOKU_DEL=60` (L217); `RENZOKU_DEL_CANT_AFTER=1800` (L219); `RENZOKU_DEL_HOURLY=2` (L221); `RENZOKU_DEL_DAILY=10` (L223); `RENZOKU_REP_HOURLY=30` (L226); `RENZOKU_REP_DAILY=80` (L228); `RENZOKU_REP_DAILY_SOFT=15` (L230); `RENZOKU_OP=yes` (L233); `RENZOKU_OP_TIME=900` (L235); `RENZOKU_REQ=5` (L240); `NO_TEXTONLY=yes` (L278); `EXPIRE_NEGLECTED=yes` (L281); `REQUIRE_SUBJECT=no` (L284); `NO_DELETE_OP=no` (L290); `NO_DELETE_REPLY=no` (L292); `CATEGORY=` (L306); `SPOILERS=no` (L406); `SPOILER_THUMB={{STATIC_IMG_DIR2}}spoiler.png` (L408); `SPOILER_NUM=0` (L410); `MAX_W=250` (L415); `MAX_H=250` (L416); `MAXR_W=125` (L418); `MAXR_H=125` (L419); `SHOW_THREAD_UNIQUES=no` (L440); `FORCED_ANON=no` (L446); `DISP_ID=no` (L455); `DEFAULT_BURICHAN=no` (L473); `SHOW_COUNTRY_FLAGS=no` (L501); `ENABLE_BOARD_FLAGS=no` (L508); `JSMATH=no` (L564); `STRIP_EXIF=yes` (L584); `OP_MARKUP=no` (L609) |

### Native default declarations

Static `Config` and `ConfigMobile` declarations in `js/extension.js:8795-8848`;
these are defaults, not a claim about a user's saved settings or completed rewrite features.

| Setting | Desktop default | Explicit mobile override |
|---|---|---|
| `IDColor` | `true` | Not overridden |
| `alwaysAutoUpdate` | `false` | Not overridden |
| `alwaysDepage` | `false` | Not overridden |
| `autoHideNav` | `false` | Not overridden |
| `autoScroll` | `false` | Not overridden |
| `backlinks` | `true` | Not overridden |
| `centeredThreads` | `false` | Not overridden |
| `classicNav` | `false` | Not overridden |
| `compactThreads` | `false` | `false` |
| `customCSS` | `false` | Not overridden |
| `darkTheme` | `false` | Not overridden |
| `disableAll` | `false` | Not overridden |
| `dropDownNav` | `false` | Not overridden |
| `embedSoundCloud` | `false` | Not overridden |
| `embedYouTube` | `true` | `false` |
| `filter` | `false` | Not overridden |
| `fitToScreenExpansion` | `false` | Not overridden |
| `fixedThreadWatcher` | `false` | Not overridden |
| `forceHTTPS` | `false` | Not overridden |
| `hideStubs` | `false` | Not overridden |
| `imageExpansion` | `true` | Not overridden |
| `imageHover` | `false` | Not overridden |
| `inlineQuotes` | `false` | Not overridden |
| `keyBinds` | `false` | Not overridden |
| `linkify` | `false` | `true` |
| `localTime` | `true` | Not overridden |
| `noPictures` | `false` | Not overridden |
| `persistentQR` | `false` | Not overridden |
| `quickReply` | `true` | Not overridden |
| `quotePreview` | `true` | Not overridden |
| `revealSpoilers` | `false` | Not overridden |
| `stickyNav` | `false` | Not overridden |
| `threadAutoWatcher` | `false` | Not overridden |
| `threadExpansion` | `true` | Not overridden |
| `threadHiding` | `true` | Not overridden |
| `threadStats` | `true` | Not overridden |
| `threadUpdater` | `true` | Not overridden |
| `threadWatcher` | `false` | Not overridden |
| `topPageNav` | `false` | Not overridden |
| `unmuteWebm` | `false` | Not overridden |
| `updaterSound` | `false` | Not overridden |

### Source inventory

SHA-256 fingerprints identify the exact inspected bytes, not source authenticity
or the current live release. Line counts use UTF-8 text split on LF/CRLF and include
a trailing empty split entry when present. The supplied checkout remains ignored;
no old source, private values, user content or additional reference file is added.

| Inspected source file | Split lines | SHA-256 |
|---|---|---|
| `admin.php` | 4389 | `4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb` |
| `auth.php` | 452 | `b300f64e6d2bc4306fa473b3a509fe38e12c292f4e6b6879c4ccf384a9ab6735` |
| `captcha.php` | 1076 | `7f054de1cca480d2c9ce656bff5cf85f85d635afafa242f4fa7192bd05023adf` |
| `catalog.php` | 554 | `9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946` |
| `config/boards/3.config.ini` | 15 | `f6a0df3ab2f49862d88f3a0f35f13bd931746865c7457d50cebc6bbcc049c3e5` |
| `config/boards/a.config.ini` | 46 | `7229450ebd103b42cdf4bec80824cbf5a0aec8491c578599698cdf0fc8a491d8` |
| `config/boards/aco.config.ini` | 28 | `29c40ba355969e1747fa52379a6c388cc6fc6660757954334b1173f1f652b093` |
| `config/boards/adv.config.ini` | 23 | `a3357ab36e444f47b7cf6eb3522519dd7f4449038ad00532380bff137d3b7c26` |
| `config/boards/an.config.ini` | 15 | `214e4a81a668d5bbebdbdf52fe1d79b872287f5c4da911ca9ad55580fa7bcddf` |
| `config/boards/asp.config.ini` | 18 | `8afc520b5a8af1676ad4aa87de1f7b7a46e1a1069953ea8a0836610a16bd3d48` |
| `config/boards/b.config.ini` | 102 | `6674a493d10c756b7c4683b3b38b17d33dc478100a333484f86f94aafa014b3a` |
| `config/boards/bant.config.ini` | 79 | `f017faabcb37871f8ecf812007418b197dd6fdfa3fc8924a9ad129cda3b50e4c` |
| `config/boards/biz.config.ini` | 27 | `b3de1bb67d0515a8dd97715fbd1b4612c27a99c736e3ebd2ef48d2583befe542` |
| `config/boards/c.config.ini` | 18 | `953f9e4eac86d1abccb2fcf42ca9c6606bb0fe58347e00e73a5443b17fdde906` |
| `config/boards/cgl.config.ini` | 15 | `aac333acb10ad5319f9db48d4947e1b293c690ed316181dc8053d2bc45274561` |
| `config/boards/ck.config.ini` | 19 | `8204571d9abd971e800e563955061ba6530477684e3cff89cded704d4c2d6736` |
| `config/boards/cm.config.ini` | 21 | `23d313b7f310c92da67746d13cd08908d46760b30be1f7fdd0aedc44d7ce7f7b` |
| `config/boards/co.config.ini` | 35 | `08d0c6994979f5cbe4ddf1a161145cdc40d85992e4b632eb505eb8c8d9c36577` |
| `config/boards/d.config.ini` | 24 | `aa838f7a77189c48ce3481cc0a0867cab9a604785bb7a0ebaff57a4a1f007465` |
| `config/boards/diy.config.ini` | 15 | `3af46136559327825dec22724fdb632555dd0e7a1a1cc73633549ee07f6d05de` |
| `config/boards/e.config.ini` | 24 | `202177b13c22c4288dea64ac96d6a53507c5c3726374e9328f9edf492321d3db` |
| `config/boards/f.config.ini` | 51 | `d9860308ce998306e08cb39a98420818f0a078da200e6a1e617672d0e169fa3a` |
| `config/boards/fa.config.ini` | 19 | `1ef9d57008020ecc1265864e61017a776920b854b683c66118963ab346dce09d` |
| `config/boards/fit.config.ini` | 19 | `0e67f18a9de472f3b89ee0f2dc19bc294cd9164027fe469619877563d820a0df` |
| `config/boards/g.config.ini` | 30 | `0c9182d9564f3ad31edcfaab4afef036f71ec9bae5e4a372a0c8be3f48c4b786` |
| `config/boards/gd.config.ini` | 31 | `518128e32bcde9206c1dadc4c27522e5ea13b2a540ac636cd2d8b2d59a6a438b` |
| `config/boards/gif.config.ini` | 48 | `ffc9372f3e6d9e5b56029138c79cd3b23efce7922227804e3aba7387fbbcb887` |
| `config/boards/h.config.ini` | 24 | `bf8356ddfe0044c51e878f335eb9e8aa7762f8c03b3b8e5e8eb0dc7b09050a2f` |
| `config/boards/hc.config.ini` | 33 | `074cdf7abeff00670f56b7015fe1f2bfad7fc0ae837228d5adf5114666dc58db` |
| `config/boards/his.config.ini` | 32 | `79de1ff38a819189b34a67a3d41defed1029d881ff6dfe3d9a1ba001c3d593dd` |
| `config/boards/hm.config.ini` | 29 | `3eca6f0fdb9f3f7105a65681158e54f39bfce3b6bca95ec2a280f51f8b36f201` |
| `config/boards/hr.config.ini` | 35 | `66b55a79119e996a009345c8f4268de5b69634a3c4c39defeff1f3018fa3c4ae` |
| `config/boards/i.config.ini` | 34 | `b587a543e5566dddde008449e8004ee66355ea95608551e96e058a1bd53facd8` |
| `config/boards/ic.config.ini` | 18 | `95d270875675408bef2ed7be30eb5ded0c6d12f969b4e65a10cfc272999bbbec` |
| `config/boards/int.config.ini` | 30 | `121bd9c9dbac8cab9df8c61826798720de6ebf09711858623767f8d2d8f678f3` |
| `config/boards/j.config.ini` | 157 | `6263d05622b725da2c56eddd0352c6cbdcdc699962a00c1237dbb11723a50ef0` |
| `config/boards/jp.config.ini` | 50 | `af02d8dd899667d7b9630f372c45a25ec5a57e0ada834cca10426985aea42c60` |
| `config/boards/k.config.ini` | 22 | `352353c92db836a78f66725b111b493a181f2aeba17fac6f00e511cb090e3ebb` |
| `config/boards/lgbt.config.ini` | 26 | `1c90a610694328cc04b02ed6f0b73c83575a32535a3581f8df5244e942ec7e63` |
| `config/boards/lit.config.ini` | 27 | `99b5eec053261573b1bc86d9280b7d13684f2dd1c958b2fbf47c90df2d097bc6` |
| `config/boards/m.config.ini` | 20 | `e79f48f4bab590d4acf14e1f60e64c74349efac38ff7fba124c8118cdec7231d` |
| `config/boards/mlp.config.ini` | 41 | `10ee30ab717c23d8bae0bcc12e4f05c91c6631605e21811698491d4ce53f94b5` |
| `config/boards/mu.config.ini` | 17 | `81d20dca2d0cb3280aae8871e16892d32a46dd4bc0a6cb3859cd5a96e1152ed9` |
| `config/boards/n.config.ini` | 15 | `ea2c774d3b854ccb6924910f2f970c3e219e6c9b65cb6da7d98dbd6569945a03` |
| `config/boards/news.config.ini` | 44 | `e0baf7bee788910a340bcd767b2df408a95ebbd9ecbf258ff99328999733a3e1` |
| `config/boards/o.config.ini` | 15 | `ebf175a84fcc8e53961fe03152d44ad8be41f53e688862e1f58d864fe40eb1ec` |
| `config/boards/out.config.ini` | 26 | `a8db9a5b94a3eae3aa2f08429be9ffe7b1e3f9d2878a9f07aea3a1512d96417c` |
| `config/boards/p.config.ini` | 27 | `9cfaa97a8b2931ebef9d1a124fb18bc3b20ad19dabf9114c26d891b0fcfc99d7` |
| `config/boards/po.config.ini` | 27 | `23277c49a9269246001141ed554073f2c4dac5d54ed72c391c6ec3d6c37dcbce` |
| `config/boards/pol.config.ini` | 122 | `40595ac1d17d4cd0547d7d154d4b5847b97aee78ba48de80e6cb6bb5e248fc6f` |
| `config/boards/pw.config.ini` | 18 | `0ecd179b93a6daf876e20aa4c67c1d64724a7fef1279dc466738ed9e10db65b9` |
| `config/boards/qa.config.ini` | 29 | `06d8a82f0e3333ac04f928f3e53113dd5996a76efffde620a88db69b6aa3cfc9` |
| `config/boards/qb.config.ini` | 17 | `3415404bd9373a8f058482ef8f9553ab17896117b146650c4b8696d0038569b2` |
| `config/boards/qst.config.ini` | 80 | `204aadc8b836ae56b91686f492780382b7cf2366a034c3fee01a88b6648b217d` |
| `config/boards/r.config.ini` | 27 | `f93ed5e781e61148fba1544604f93a4dc4fe1be2edec618cfb41947aa5818f12` |
| `config/boards/r9k.config.ini` | 43 | `fcbb1f7dbd59de8b5b259c87f3a7e338f8ba0b8e113c9e47a93de3fb5d9cbc50` |
| `config/boards/s.config.ini` | 35 | `f450029f53e00ca98986861011541b72e3bcb47fdb8951bd509e19ff06671fb3` |
| `config/boards/s4s.config.ini` | 80 | `529d818119af6a0b6b74252ac82d531cb97389cc0f020dfb800adbf163353a82` |
| `config/boards/sci.config.ini` | 34 | `53cd956f750a4a8923c4d7f7c61d48e432cd4b528346627041816b8c3743de18` |
| `config/boards/soc.config.ini` | 52 | `fa6f7ed52751d110472457ce124fe10588b7a3138ea8d39011c744f68d2303be` |
| `config/boards/sp.config.ini` | 41 | `7d29bca3fe97434022213059e4e81fcfe3276c210d3b8cb7ad39f36437b4336f` |
| `config/boards/t.config.ini` | 22 | `797e321b2f4761c040d705fd26202fb6fe944d2ac1935a0e5b502d50da87ab63` |
| `config/boards/test.config.ini` | 218 | `72f1ddef8a63944bfb40e5d2e368c8ffeb42bef4c18b02a04d3638186a98b683` |
| `config/boards/tg.config.ini` | 47 | `caaa089d0a490ce76062b3bacd431e23780f2739e27579f250290cc0a8af2e13` |
| `config/boards/toy.config.ini` | 19 | `0524258e4e7fd0d08e540aafd508d5eef22bb5748c619b20d597dbb44259ae4d` |
| `config/boards/trash.config.ini` | 43 | `1079782591e9b7c428931376d426e83ab3aa7414b437b4a1fc5d2b77bc006304` |
| `config/boards/trv.config.ini` | 28 | `e29556a20680f938d7b69697c0f70c8a75b2f51f61776534adc262197d4adfcf` |
| `config/boards/tv.config.ini` | 36 | `a4ad9e9c6013beef91a52338f7300ddd5fc324731fdcf49a40ab19bea801015e` |
| `config/boards/u.config.ini` | 28 | `a05624b3336597cd7bcfdfc429c238dfa38da9f9a3051a164ae98d9a5f7059d7` |
| `config/boards/v.config.ini` | 60 | `d6607710b642e566827bcdc40576b29853fd8328feac3c5fd04816f7019130c5` |
| `config/boards/vg.config.ini` | 65 | `be3871b58be00f8fc1baafefc3eb43141865df4832c6a749d6bb0d9b5f98a050` |
| `config/boards/vip.config.ini` | 42 | `39515e6dc058310314425508c65901dc1e6dd38648d76f1954786a2bd0f008f3` |
| `config/boards/vm.config.ini` | 40 | `beff9c061bf350b310399272c984a2bdda3e2a26fff9bcb75469f123386f0b84` |
| `config/boards/vmg.config.ini` | 40 | `7eea31627c25c7d11ba013cbe12599b2382e579b224317f313b4728b17e63b93` |
| `config/boards/vp.config.ini` | 27 | `4dedb8f011f75a94d66611d62bf0b92aeb1bfa76ed16e1c55972aa26822116a0` |
| `config/boards/vr.config.ini` | 33 | `abc9b280939168ba7341a75543c340b6138a7d9d05db3a8d0675edef92ba6c5e` |
| `config/boards/vrpg.config.ini` | 40 | `df6d653b75a712e0fbe5a37bd098a53ea356105ef6b3bbcefceea7bb3a5fa0a1` |
| `config/boards/vst.config.ini` | 40 | `eb19acf68d9981bb8793b526cc0d2a42758c60e5b271968ee34c849fd9ea936b` |
| `config/boards/vt.config.ini` | 48 | `3023e58c0025584276f3a1d80d84e01bc55779458690122c8d9d231d4814c4e2` |
| `config/boards/w.config.ini` | 32 | `eee2c5cd258ef054689bb9efde18580b84524e4e89d44d4de8484970c7aca6ca` |
| `config/boards/wg.config.ini` | 32 | `2fb95463ac8087d98cc545782a7722cf624ba864ec02e5d22516b521a18f5bf3` |
| `config/boards/wsg.config.ini` | 45 | `b8d30a18b219eaaa6f2ac1d1ae0a04ad2fe70ddaf39b63a3d8b3bb2e89e796e9` |
| `config/boards/wsr.config.ini` | 24 | `e81f3956de4177f7f5fd96ad9931e7effcdfd1faa32266cd3e21767543e62700` |
| `config/boards/x.config.ini` | 15 | `76b289f89b7fa27d14b01158241f5c299fdd5cf55214e31411846edcfa7735ac` |
| `config/boards/xs.config.ini` | 15 | `00358d9b5b2c250b89e49eca95c5ed10129ba077d8f437ffec222ac781dbcd02` |
| `config/boards/y.config.ini` | 24 | `6a1a80070ba2e59c9f88bef673ab6d3bf414bfa359af59dfe1f7b963d5fd6a65` |
| `config/categories/nws.config.ini` | 99 | `9bddef7e608991689ccf8bbb393461db4606445f8dfaa3be62597325729fb9a4` |
| `config/categories/ws.config.ini` | 95 | `9e076a26b1b7bf35b51219aa13a9fc03baafe8c099d67fda154f7259d97e8585` |
| `config/global_config.ini` | 665 | `8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d` |
| `css/burichannew.css` | 1683 | `71a5a5b8362aa78269c19e89cacc249cda48f2af2393fda7d3638fa518307b18` |
| `css/catalog_mobile.css` | 397 | `8c0774fd8f9b3644ad88b87a0203184b070cee38f1cfdda33f9f75f9a58848d0` |
| `css/futabanew.css` | 1677 | `8beb159cfcf83d042a68b1037997511c7b0bb959289eb1b5757036cdd7aa0945` |
| `css/photon.css` | 1707 | `a4741759760d959b4a2e29d9622ab63a91d1cdc49f10679335b71f5b16e9575d` |
| `css/tomorrow.css` | 1735 | `40b4c474a93e87737811c77a50cce9af4fadc9b025a3863e126ca418a8f33a4a` |
| `css/yotsubamobile.css` | 1036 | `872810133bc5dff715746b30127d531e7cd6d93d8405781af23dc7c4760eb023` |
| `css/yotsubanew.css` | 1732 | `05d61b9a886aca60bd418b63a5b86d2ef3050d1d90c640da64f2206dda740db9` |
| `css/yotsubluemobile.css` | 1031 | `3cb6eb4ec8fcb261b2f0496390484fa613d27ec1bad26fc97127b7dcb848d6ff` |
| `css/yotsubluenew.css` | 1734 | `d6abc7c9fb8c2d9c168554b39a54b4569f68243aa4bcb972b8452264cbdd1a65` |
| `forms/report.php` | 206 | `282ea7b12c8b1d2dc01e25e1dcbed0d09daae6c63e697831242d8c08d186633c` |
| `imgboard.php` | 10403 | `caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445` |
| `js/catalog.js` | 3613 | `12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e` |
| `js/core.js` | 2568 | `a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5` |
| `js/extension.js` | 11228 | `05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31` |
| `json.php` | 720 | `18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868` |
| `lib/admin.php` | 604 | `571d154aa43cd51ee17bb4dfec5502d6dd63c265f4a2526820c011c934c7d185` |
| `lib/archives.php` | 172 | `de8a121be2e84b31c208a8c47a3713ba568432a416f5d95781678ca37a10a703` |
| `lib/auth.php` | 595 | `98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019` |
| `lib/captcha.php` | 687 | `1852aaa5978b54ef850479eca326c87d47475d8da25b8c10e3e4be649dd3182e` |
| `lib/db.php` | 346 | `f854b6a2a036f12cd5503825c646cdc8168d49e07560edbac5e83af7dd9a1915` |
| `lib/ini.php` | 125 | `05a89bb56d627f9f5d0a07f3200a3efc9fbe50e6d84eb7e68e86d012fb318567` |
| `lib/json.php` | 14 | `7da323a8a3798dded6b8a865361604343071bc6eb5e6f6664bc77c06a8f73386` |
| `lib/postfilter.php` | 3916 | `d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207` |
| `lib/userpwd.php` | 957 | `0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26` |
| `modes/report.php` | 666 | `e52c0042295c942c12bfbfa404ca02b14932309fecd8aa9c2b3cc298aefa91ae` |
| `signin.php` | 1042 | `f646724efd291744fc8b4e46fda4f67c069277842dd7798512f864142f18cda2` |
| `views/imgboard.php` | 739 | `6c2771682a20672f4f8395e78a2cb9fa80a9ed55933b77a0fcc54cc9f23e17eb` |
| `yotsuba_config.php` | 123 | `c029e1d016e9b6e575c796784d0c7ccb3b89f80bceac0983d4468fcdf1a1e562` |
