# Poster-ID display qualification

This follow-up extends PR #227 with the
[source sage display policy](source-poster-id-display.md). Complete source
behavior and frontend parity remain the completion requirement.

Migration 0074 has SHA-384:

```text
c1a085cec4468a59238e06c712df240443ed70ddaba7bd281022409a2eb0de089a1bce29aca2f6a9b18701d3265990e2
```

Migration 0075 has SHA-384:

```text
b711c8a54501209c5395349c623ec86781f1b5606b86d8a0782c4931b7568f06729c88c32a655a058e31694124b6017b
```

Migration 0076 has SHA-384:

```text
34addb2ceeb20704844fcdd72fdaa5fc6c970fdce0f127f5b43cebb37bdb4db29f9351389c9f94c13ce92e3c18dcdac6
```

All three files were applied to the owned qualification database and remain
unchanged. The database has 67 successful migration files through version 76.
The historical 0045 board import and 0047 RSS migrations still regenerate
byte for byte after adding the new source policy to the reference.

| Check | Result |
|---|---|
| Independent source display reference | Passed; 112 cases under PHP 8.3.6 |
| Independent source JSON projection | Passed; 448 cases, including the ordinary sage OP exception |
| Board reference and historical migration regeneration | Passed; 82 definitions |
| Populated 0074 upgrade | Passed; all 112 labels through public/staff roles, historical fields and existing proof preserved, replay and forgery denied |
| Populated 0075 upgrade | Passed; 112 OP and 112 reply cases, public network capture, unchanged history/proofs/privacy policies/owners/grants, runtime column denials |
| Public unit suite | Passed; 114 cases, including every independent JSON projection case |
| Public ID, poster-count and inventory HTTP/PostgreSQL suites | Passed; 4 cases, including both JSON listeners, HTML, policy lock wait and archives |
| Staff HTTP/PostgreSQL suite | Passed; 18 cases in 3.02 seconds |
| Populated 0076 upgrade | Passed; reproduced the denied owner insert before the grant, preserved history and metadata, enabled only two policy reads |
| Real attachment store suite | Passed in 12.49 seconds; atomic upload use, source labels, OP network capture, suppression switches and private counts |
| Focused client unit/DOM/browser suite | Passed; 97 cases including Heaven colors, highlighting and loaded-post counts |
| Actual public browser ID workflow | Passed; both network and sage cases against the public service and PostgreSQL |
| Quick Reply transport and response capture | Passed; 16 cases, including real Chromium upload/post bodies after transport cleanup |
| Browser diagnostic redaction | Passed; 8 cases |
| Persisted flag browser workflow | Passed in 11.38 seconds after correcting response capture |
| Fresh role bootstrap | Passed; all 67 files, ownership and runtime/private access guards retained |
| Strict workspace Clippy | Passed in 47.54 seconds; all targets/features, warnings denied |
| Complete workspace and remaining regression gates | Incomplete locally; the foreground scope disappeared during compilation |
| Ordinary sage OP's JSON network-label exception | Passed; both listeners expose the network label while HTML retains Heaven |
| Hosted checks at `bdb8fe5` | Windows visuals, monitoring and advisories passed; Linux browser and media failed on response-body reads |
| Hosted checks at `5741bef` after the response-capture correction | Passed; complete Linux Rust/database/browser, Windows visuals, media/operations, monitoring and advisories |

The first published `fdb6f2c` head failed hosted build/test. Linux backfill and
the real upload qualification exposed missing board-column reads for the
restricted attachment function owner. Migration 0076 adds only those reads;
the populated upgrade and real attachment store rerun pass. Readiness now
checks the grants. Monitoring and dependency advisories passed at that head.

Windows media visuals passed 67 cases and failed the catalog's large-teaser
case while loading the file-deleted placeholder. The retained failure image
shows that broken image; the original artifact omitted the network trace, so
its cause remains unresolved. Failure artifacts now include traces and error
context. Screenshot baselines, retries and deadlines remain unchanged, and
the next hosted run must qualify the complete Windows suite. That complete
Windows job passed at `bdb8fe5`; this does not identify the earlier image-load
failure's cause.

The `bdb8fe5` media job passed the real PNG, JPEG, GIF, tracking and approved
Quick Reply uploads. Inline Quick Reply then failed while reading an HTTP 200
JSON response through Playwright. The Linux flag browser failed with
Chromium's missing-response-body protocol error. Quick Reply cancels its
fetch controller after consuming the response, which can discard the body
before the external browser listener reads it.

Both fixtures now capture a clone inside the page before returning the same
response to the actual client parser. This uses the approach already present
in the Quick Reply suite. The Chromium regression exercises the real upload
and posting transport, verifies both clients' parsed results and observes
exactly one request per operation. Existing status, receipt, persistence,
replay and deletion assertions remain. The 16 focused transport/response tests,
8 diagnostic-redaction tests and actual persisted flag browser pass. The
complete Linux and isolated media qualifications remain required at the next
published head; these local checks do not replace them. Those complete jobs
subsequently passed at `5741bef`, together with Windows visuals, monitoring
and advisories. The [build/test run](https://github.com/frankischilling/26chan/actions/runs/37118442290)
includes all application and privileged qualification jobs. That result
qualifies the response-capture checkpoint, while complete source behavior and
frontend parity remain unfinished.

The new Heaven color/action tests first failed against the prior controller,
which accepted only eight-character IDs. After adding the exact source label,
all 97 focused client checks pass. Invalid case, trailing whitespace and line
break variants remain rejected. The real public browser fixture now runs both
network and sage cases. Both passed locally against the actual public service
and PostgreSQL, including previews, live updates, filtering, colors and ID
controls. The focused run took 2.5 minutes including server startup. Complete
hosted qualification remains required.

The first HTTP fixture requested a tail size of 20 for a thread with fewer
replies. The existing endpoint correctly returned 404. The fixture now uses
two tail replies and verifies those selected saved labels. The failing log
is retained; endpoint thresholds and test deadlines were not changed.

The first bootstrap comparison checked migration 0072's board snapshot after
all later migrations, so 0074's new policy column caused a mismatch. The same
0072 SQL assertions now run immediately after that migration. They continue
to compare exact prior fields, privacy policies, owners and grants. The
separate 0074 upgrade checks the new source defaults and denies runtime
updates of the policy column. The bootstrap's historical post comparison
excludes the new 0075 column and separately requires it to remain null on
historical rows. All existing fields remain in the exact comparison. The
corrected fresh bootstrap passes.

The earlier 0074 Clippy run passed in 2 minutes 6 seconds after an interrupted
run. The first 0075 Clippy run found two visual fixtures that supplied the OP
field to a reply-only structure. The constructors were corrected; the failing
log is retained. Qualification uses a foreground scope with a verified 3 GiB
limit and the same local profile without debug symbols.

The first populated 0075 fixture had a PL/pgSQL expression syntax error; its
next run incorrectly bound an OP proof to thread zero. The fixture now binds
the reserved OP number as its thread ID, as the real writer does. The existing
positive-thread proof constraint was preserved. Both failed logs and the
subsequent passing upgrade are retained.

The subsequent full-workspace scope disappeared during compilation and left
no final result record. Its partial log is not counted as a workspace pass.
Source review confirmed that `json.php` recomputes an ordinary OP's network
label while replies retain the saved label. Migration 0075 and the HTTP fixture
now cover that distinction. The historical `Heaven` OP limit is stated in the
[source policy](source-poster-id-display.md): without a previously retained
network label, JSON omits the ID. No historical address is reconstructed.

The focused runs use the same local profile without debug symbols and the
owned qualification database described in
[static staff ID qualification](verification-staff-poster-ids.md). The earlier
local full-suite interruptions are retained there. These focused checks do
not establish a complete workspace or browser pass. The preceding published
`fb95886` static-ID checkpoint has its own hosted results; it does not qualify
this follow-up.
