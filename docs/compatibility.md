# Compatibility target

The pinned input is the public [4chan read-only API documentation](https://github.com/4chan/4chan-API/tree/2bd670d507ba2daa37a3961a661e088cf6f89d57), revision `2bd670d507ba2daa37a3961a661e088cf6f89d57`, collected September 8, 2026. [reference-manifest.json](reference-manifest.json) records exact source URLs and SHA-256 hashes. Files were fetched as public documentation; no live user posts, media or private source were imported. The input does not specify posting, authentication or private moderation behavior.

No original visual or interactive reference snapshot has been collected. No independent design-brief attachment was available. Visual/behavioral parity remains unresolved. The current CSS is an original implementation of a compact, warm-colored board layout. Paperboard is not presented as an official service.

Evidence classes: **documented** means established by the pinned API docs; **project** means a deliberate local behavior; **unknown** means reference evidence is missing. Status "implemented" applies only to the stated local scope, not universal client compatibility.

| ID | Scope and evidence | Status | Tests / exception |
|---|---|---|---|
| I-001 | `/boards.json`; documented `Boards.md` | Partial: required settings, integer switches, text-only flag | Public HTTP API; media sizes zero, byte-limit and cooldown exceptions below |
| I-002 | `/{board}/thread/{id}.json`; documented `Threads.md` | Implemented text-post subset | `public_flow`; numeric `no/resto/time`, escaped `com`, OP reply/image counts, conditional subject/sticky/closed/bump flags; no media, unique-poster count, capcodes, trips, flags |
| I-003 | `/{board}/threads.json`; documented `Threadlist.md` | Implemented page groups and visible-reply counts | API regression coverage; SQL aggregates avoid loading comment bodies |
| I-004 | `/{board}/{page}.json`; documented `Indexes.md` | Implemented OP + latest five replies, omission counts | API regression coverage; positive 1-based JSON pages |
| I-005 | `/{board}/catalog.json`; documented `Catalog.md` | Implemented OP + latest five replies, modification time | API regression coverage; no attachments |
| I-006 | `/{board}/archive.json`; documented `Archive.md` | Not implemented; 404 | No archive enabled or advertised |
| I-007 | Conditional JSON responses; documented API guidance | ETag implemented everywhere; Last-Modified/If-Modified-Since on individual threads | Cache unit/integration tests; metadata/posts use one database snapshot; same-second date requests conservatively revalidate |
| I-008 | Public HTML routes and DOM IDs; project, original reference unknown | Board index, zero-based numbered HTML pages, thread/post navigation, catalog, `t/pc/p/pi/m` IDs | Browser behavior and visual tests; no claim these cover every client selector |
| I-009 | Legacy-looking posting endpoint; project | `/{board}/imgboard.php` POST alias handled by Rust | Same form contract as `/post`; undocumented original posting parameters unsupported |
| I-010 | CORS, redirects, status/header details; partial documentation | Same-origin public application; GET/HEAD routes, 303 posting, 308 board slash, bounded errors | Browser/HTTP tests; separate API-origin CORS/OPTIONS contract remains unfinished |
| B-001 | Persistent thread creation/replies; project | Implemented | Real PostgreSQL and browser tests; no copied posting internals |
| B-002 | `sage`, bump/reply limits; project implementation of inventoried concepts | Implemented, serialized per board | Concurrent reply test; lifetime counts do not decrease after deletion |
| B-003 | Board settings and thread limits; project | Byte limits, active-thread cap, reply/bump limits | Domain/concurrency tests; full boards reject new threads instead of automatically pruning |
| B-004 | Deletion; project | Argon2 password; OP deletion hides whole thread | Wrong credential/origin, absent store, persisted deletion tests; no staff identity involved |
| B-005 | Greentext, local quote navigation, spoilers, HTTP(S) links; project grammar | Implemented as nonrecursive typed nodes | Unicode properties, escaped rendering and browser tests |
| B-006 | Reports; project | Validated reasons persist; no review workflow yet | Browser reporting; no false claim of staff review |
| B-007 | Staff moderation/authentication; project requirement | Not implemented | Separate schema grants tested; WebAuthn, sessions, CSRF, audit and recovery remain open |
| M-001 | Media intake/publication; project security requirement | Disabled; startup rejects enabling it | Startup and unsupported-upload tests; processing containment unverified |
| V-001 | Desktop/mobile board, thread/form spacing and typography; unknown reference | Original layout; three synthetic Windows baselines | `visual.spec.js`; 1280x900 and 390x844, scale 1, locale en-US, light scheme |
| V-002 | Additional themes, media thumbnails/spoilers, archives, loading/error/empty snapshots; unknown reference | Not complete | Basic empty/error views exist; comprehensive reference screenshots unavailable |

## Security-driven and project-defined exceptions

| ID | Reference/old behavior | Replacement and reason | User impact and test |
|---|---|---|---|
| E-001 | API documentation describes `.jpg`, `.png`, `.gif`, `.pdf`, `.swf`, `.webm` attachments | All upload/processing/original-download paths remain disabled until a tested isolation and promotion tier exists | Text-only posts. No originals retained or downloaded. Startup/upload tests. Future original downloads require explicit policy; worker containment does not make downloaded files safe in every client parser. |
| E-002 | Raw HTML acceptance is not established by permitted posting documentation | User text becomes typed formatting nodes; Askama escapes every text/attribute | HTML appears as text. Unicode and real-browser hostile-text tests. |
| E-003 | Public staff/password behavior is not specified by read-only API docs | No staff login exists; public author deletion uses a local Argon2 password policy | No moderation UI or weaker staff fallback. Protected staff/deployment schema denial tests. |
| E-004 | Thread API documents `unique_ips` for active OPs | Omitted; persistent IP tracking is not implemented | Clients expecting that field need an exception. No invented count. |
| E-005 | `max_comment_chars` is described as characters; media capacities as positive values | Limit enforced in UTF-8 bytes; media capacities returned as zero alongside `text_only: 1` | Multibyte text can reach the limit sooner. Domain byte-limit test. This is a partial settings contract. |
| E-006 | Exact original posting cooldowns/deletion/concurrency rules are unknown | 30 writes per peer per minute, 32 concurrent requests, 4 Argon2 operations; active thread cap rejects overflow | Transparent local limits; no bypass through forwarded headers. HTTP and concurrency tests. |
| E-007 | Original retention behavior is unknown | Deleted text remains in PostgreSQL but disappears from public routes; reports persist | Compromised public database credentials can read retained content. Operator retention/erasure policy is a launch prerequisite. |

The synthetic fixtures and screenshots are project-owned test data. Screenshot changes require review in the recorded browser/platform/font environment. Current baselines were generated from the database-backed application, visually inspected, then checked against the separate renderer using the same production templates.
