# File-blocking parity

Exact-file rules are connected to public and staff posting. The earlier
prior-ban upload gate still needs a qualified history provider. Trusted
publication records the original upload's MD5 in a private receipt.
Existing SHA-256 integrity checks remain unchanged. MD5 is only a compatibility key for
matching operator-owned rules; it is not an approval or integrity proof.

## Original and processed bytes

`InputSnapshot::raw_md5()` hashes the same immutable input bytes as its SHA-256.
`PairedInputSnapshot::raw_image_md5()` hashes only the uploaded image component,
after validating the frame and component descriptors. It excludes the replay
and framing. Neither accessor reopens a path or hashes worker output.

The existing `SourceProvenance.md5` describes processed bytes according to its
source profile. It cannot be repurposed as the untouched input digest. Durable
raw-input metadata has its own private, lease-bound receipt and exact retry
checks. Old uploads remain unknown rather than acquiring an inferred digest.
Approved media views do not expose the private metadata.

## Durable receipt

`media.input_receipts` belongs to the reserved asset, not its temporary queue job.
It stores the original SHA-256, byte length, MD5, and component kind. Queue cleanup
does not remove it; deleting the asset cascades to the receipt.

The trusted host records it under the publication filesystem lock after reserving
the asset and before installing files. The writer locks the job before the asset
and requires the current processing lease, matching byte length, and a pending
asset. Drawing receipts additionally match the stored image-component SHA-256.
Existing processed-source provenance, when present, must agree on input identity.

The production approval paths require an existing exact receipt within their
approval transaction. They cannot fill a missing receipt. A failed or uncertain
write leaves the reservation recoverable and prevents file installation; an
uncertain approval preserves files for reconciliation. Exact approved-receipt
verification is allowed after queue cleanup, but historical approvals cannot
acquire a new receipt. Manual publication APIs without an original snapshot
continue to leave original identity unknown.

Both private functions have fixed search paths and the migration owner. Runtime
publication receives only function execution, with no direct receipt-table grant.
Public posting, staff, intake, and media-reader roles cannot read the table.
Posting roles have a separate capability-scoped metadata function; it is not an
HTTP endpoint and does not add fields to public media views.

## Posting preflight

Before content-rule evaluation, every attachment passes a read-only preflight under
the board, reply-thread, and media-job locks. It checks the live upload capability,
approval, board media limits, reply image quota, PDF/video consistency and the
stored still-image context. Staff identity comes from validated posting authority,
not upload flags. The final proof handoff and one-use attachment consume remain
in place.

A file that cannot attach must not create a filename ban or other committed
content-filter effect. Rejection and quiet-success paths therefore revalidate the
metadata and capability expiry immediately before committing their effects.
The separate automatic-range hit also revalidates at the refusal decision,
before rolling back unrelated posting effects. Preflight itself does not attach
the file or consume its capability.

The scoped result includes normalized MD5, nullable original MD5, and nullable
original-digest eligibility. `/p/` JPEG and PDF are ineligible. The capcode-only
JPEG strip on `/p/` changes normalized bytes but does not enable original fallback.
A generic PNG output can have originated from a different format; without a
retained-format manifest or the PNG source profile, eligibility stays unknown.
Later blacklist decisions must distinguish unknown eligibility from false.

## Required posting behavior

The original posting path has two distinct checks:

- Before content filters, the prior-ban upload check uses the normalized file
  digest. It skips `/b/`, blocks after three matching qualifying bans, and includes
  the work-safe-only template when appropriate. It has no capcode exemption.
- After content and network filters, the exact file blacklist checks the
  normalized digest first. Only a miss can fall back to a different original
  digest. A trusted active capcode bypasses this second check.

Original-digest fallback is format-dependent. The original implementation records
it before WebM/MP4 validation and PNG/GIF stripping; JPEG records it only when
EXIF stripping is enabled. PDF does not supply an original digest. Capturing a
raw digest for provenance does not authorize using it as fallback for every
format.

Exact matches can reject, issue an automatic ban, or enter the DMCA warning and
repeat-attempt path. The DMCA path precedes quiet fake-success handling. Those
effects use the capability-scoped finalizer described below. The original query has no ordering
for overlapping matches, so a new scope priority must not be presented as an
established source behavior.

## Snapshot qualification

The media suite checks a fixed `abc` MD5 vector, unchanged snapshots after a path
or inode is overwritten, and identical image digests across absent or changed
replays. It also checks that a frame digest differs from the image digest. These
tests qualify snapshot capture. Database tests separately exercise receipt
privacy, lease expiry, exact retries, approval refusal without a receipt,
historical backfill refusal, paired-component checks, and cleanup. Controlled
TLS dispatch tests compare frozen raw-input digests independently of the
processed-source digest. They do not qualify blacklist enforcement.

## Bounded decision model

The domain model selects candidates for the private effect finalizer. The
reference fixture exercises 2,358 exact-file decisions from the pinned checker
and 600 prior-ban cases. The SQL and effect callbacks use synthetic data; they do
not run the application or import private lists.

Exact matching tries normalized MD5 first. A different original digest is tried
only after a miss and when eligible. An unknown provider does not override a
definite normalized match. Unknown original eligibility matters only when the
available original digest matches a candidate rule. Badge exemption belongs to
the exact checker, not the earlier prior-ban gate.

The prior-ban gate skips `/b/` and known absence of a digest. It blocks at three
matching historical template-226 records, also counting template 6 on work-safe
boards. These are history counts, not only active bans. Of the 600 source cases,
100 demonstrate the source's failed-query allow behavior; the Rust model instead
returns unavailable when that query is needed.

The DMCA branch ignores quiet mode: a first warning records an attempt, while a
repeat requests a three-day ban and redirect. `/test/` rejection diagnostics
precede quiet handling for reject-only rules. The finalizer separately checks exemptions, expiry dates, identity binding and
the inclusive one-day warning window before committing a ban. The
fixture also covers 24 failed DMCA-history queries: the source takes its first-
warning branch, while Rust returns unavailable without requesting an effect.
Exact-rule query failure is not part of this corpus. The caller must propagate a
failed rule fetch, never replace it with an empty successful rule set.

Rules are capped at 128, IDs must be unique, scopes are validated, and configured
ban-day requests are bounded to -1 through 3,650 (-1 means permanent). Two
applicable rules for the same digest produce a configuration error. The legacy
query's unordered `LIMIT 1` supplies no stable scope priority; the replacement
does not invent one. Missing needed history or digest providers fail closed.

## Operator rule storage

`admission.file_rules` holds operator-owned MD5 rules. Migration 0184 starts with
an empty table. Runtime roles cannot read or change the table or its sequence.
The scoped `content.file_admission_rules` reader returns active global, board,
and applicable work-safe rules for a public board. Private reasons and complaint
descriptions stay outside its result.

The reader holds the shared admission-generation lock until its transaction
ends. Operator edits to text, network, or file rules must wait for that snapshot.
Posting callers must already hold the board lock, as they do for other admission
stages. The reader requires read-committed isolation and returns at most 129 rows;
the extra row makes a policy above the domain's 128-rule bound fail closed.
Ordering by ID makes snapshots reproducible but does not resolve overlapping
matches. The domain model still rejects an ambiguous match.

Rule modes are reject, automatic ban, and DMCA warning/repeat. Storage validates
scopes, digest length, modes, reason sizes, and ban-day bounds. Ban days apply only
to automatic-ban rules; the DMCA branch has its own fixed repeat duration. The finalizer rechecks these settings before issuing a ban.

## Private DMCA warning state

Migration 0187 adds the private warning-state operation used by the file-effect
finalizer. Runtime roles cannot call it directly or read its tables. The helper
records warning state; its caller decides whether to issue a ban and redirect.

The source's active checker queries `fail_dmca` by exact peer across boards,
files and rules. A warning in the inclusive preceding day makes the next match
request a three-day global ban. Repeats do not extend the warning timestamp.
Top-level `admin.php` deletes `user_actions` older than seven days. No other
consumer of these DMCA rows was found in the supplied source.

The replacement retains one timestamp per server-derived exact-peer hash,
without a raw address, password, filename or post body. This preserves the
repeat decision while discarding the older per-attempt audit history. An actor
lock serializes simultaneous first attempts, including when no row exists.
The helper requires read-committed isolation and a trusted decision clock within
30 seconds of database time, checked again after lock waits. The finalizer
supplies a database-derived timestamp and server-derived peer identity; public form fields
and timezone-naive client strings cannot provide them.

Its capacity defaults to 100,000 entries and is bounded at one million. New
entries lock the capacity row and prune at most 256 unlocked warnings strictly
older than seven days. Existing repeats leave their timestamp unchanged, even
at capacity. A missing capacity policy or exhausted capacity refuses a new
entry without partial writes. Lowering the limit below existing usage can
require operator cleanup before new entries fit. Operator capacity is separate
from the existing content-rule limits.

The helper pins UTC for deterministic one-day and seven-day intervals. The
checked-in source does not declare the production PHP or MySQL session timezone,
so this is not evidence that the original deployment used UTC. Automatic-ban
expiry remains separate: the source computes a local-midnight date, and that
issuer requires an explicit operator timezone policy.

At the 0187 checkpoint, 37 local database tests passed, including the new warning-state
cases and existing content, staff and refusal-cookie regressions. Tests cover
the inclusive repeat boundary, unchanged timestamps, actor concurrency, clock
and role denial, finite capacity and the 256-row cleanup bound. Populated
rollback/upgrade, historical cleanup/restore, strict all-target/all-feature store
Clippy, formatting and five workflow-helper tests pass. Hosted qualification of
this checkpoint is still required. These checks do not qualify live file-rule
enforcement or ban issuance.
## Automatic-ban dates and identity

The isolated issuer fixture executes the pinned `auto_ban_poster` function in
420 synthetic cases. SQL, reverse DNS and the clock are replaced by local
callbacks. It covers permanent, zero-day and finite requests, whitelist
suppression, optional identity forwarding, midnight, leap day, year rollover and
US daylight-saving transitions. UTC, New York and Tokyo are explicit test
settings; none is presented as the original deployment's timezone.

The issuer adds whole multiples of 86,400 seconds, formats the resulting local
date, then stores midnight. It does not add calendar days or preserve the time
of day. A request of -1 stores the permanent zero-date sentinel. Other negative
requests clamp to zero in the source; native file-rule configuration rejects
those out-of-range inputs.

The insert omits `active` and `now`. The supplied schema defaults them to active
and the database clock. With aligned PHP/database timestamps, a zero-day request
therefore becomes a warning: the posting checker tests whether expiry is at or
before the start before testing ordinary expiry. Equality also warns. A warning
stays a warning on later requests until acknowledged or retired. A database start
before that expiry would instead give the ordinary finite-ban/expiry behavior;
clock and timezone alignment cannot be assumed for an unknown deployment.

The Rust regression feeds the captured dates into the separately qualified ban
policy. It checks warning precedence, permanent bans and finite expiry, including
which rows may be retired. It does not implement or qualify a native issuer.

The source forwards the supplied UserPwd and Pass values into its insert. All
fixture values are synthetic. The native writer must bind only a legitimately
resolved server-side identity, or leave it absent; it must never persist the raw
credential or invent a user identity to satisfy a foreign key. Whitelist
suppression applies to the ban insert and does not by itself suppress the
file-checker's warning, refusal or redirect behavior.

## Private restriction provenance

Migration 0188 extends the existing restriction table so a future file issuer can
use the same IDs, public status, warning acknowledgement, unban and audit paths.
It adds no runtime issuer or direct table grant. That migration alone did not connect live matching or effect recording.
Migration 0192 supplies the posting finalizer.

Existing rows retain staff provenance. Staff restrictions still require a
positive post ID and staff account ID, and cannot carry file-rule metadata.
File and DMCA provenance instead require a positive historical rule ID and its
16-byte matched digest, with no post or staff account. These records describe a
rejected attempt, so creating a fictitious post or staff account would be wrong.
Rule IDs are retained without a foreign key to the editable operator rule table;
they are historical evidence, not proof that the rule is still authorized.

Staff warnings still require equal start and end timestamps. An automatic
warning may end before its start, preserving the source's midnight behavior.
Ordinary finite restrictions must end after their start. Both kinds use the
existing actor-bound warning acknowledgement and audit record. Only the public
reason is returned by status; private reasons stay in private storage.

Posting policy derives automatic provenance from the stored origin. Its
case-sensitive `Proxy` exception for verified users examines both public and
private reason text, as the source does. It applies only to address matches;
a matching retained automatic identity is checked first. Staff rows cannot
acquire that exception through their reason text.

The matched rule digest does not establish a qualifying historical template or
the upload's normalized digest. Prior-ban upload counts still need their own
qualified provenance. The 0192 finalizer enforces current rules, exemptions, finite capacity, trusted
identity, timezone policy and attachment revalidation before committing effects.

At the 0188 storage checkpoint, 51 local database tests passed, including
provenance constraints, real public status and acknowledgement, verified Proxy
controls, existing restriction integration, quota, staff posting, content
admission, refusal cookies and DMCA warning state. Populated rollback/upgrade,
historical cleanup/restore, file-deletion preservation/restore, strict store
Clippy and formatting also pass. Hosted checks must still qualify the exact
pushed head; these results do not complete the file-effect workflow.

## Private file-ban persistence

Migration 0189 adds an owner-only persistence operation and an explicit-timezone
expiry helper. Public, staff and authentication connections cannot execute them.
Migration 0192 connects them to posting through a capability-scoped finalizer.

The operation takes the actor lock, exclusive restriction-capacity gate and
public-board lock in that order. It must run in a fresh effect transaction after
the posting transaction has rolled back. It uses the database clock, resolves
only an existing valid anonymous identity, and stores a global file restriction
with its audit entry and capacity charge in one transaction. Unknown or expired
tokens leave identity absent. A retained identity remains usable during a
creation-second or idle reset; the helper never creates one. DMCA-origin requests
must use the source's three-day duration.

The expiry helper requires a named database timezone. It adds fixed seconds,
then truncates the resulting local date to midnight. Permanent requests retain
no expiry; zero-day results become warnings. The non-exempt cases in the
420-case source fixture check the captured dates, while out-of-range durations
and unknown timezone settings are rejected. A fresh installation leaves the timezone unset. An operator must select it
before a non-exempt file rule can issue a ban.

The private operation accepts already-qualified effect facts. It does not prove
that an actor owns an upload, that a rule is current, or that an exemption does
not apply. The 0192 finalizer validates those facts, rechecks the attachment and policy,
and chooses the public response. The Store does not blindly repeat an uncertain
commit. Capacity contention
refuses the transaction; it must never turn a rejected file into an accepted
post. Prior-ban history qualification remains unfinished.

At the 0189 private checkpoint, 55 local database tests passed. The new cases check
source-derived expiry dates, capacity refusal without partial effects, real
runtime-role denial, isolation-level refusal and contention followed by a fresh
retry. Populated rollback/upgrade and both historical-erasure and file-deletion
administrator restore checks pass, as do strict store Clippy, formatting and
five workflow-helper checks. Hosted qualification remains required for the
changed head before any merge.

## Private effect context

Migration 0191 adds three admission-owned functions used by the restriction-owned
finalizer. They are private cross-owner calls: posting runtimes cannot execute
them, and the restriction owner gains no direct access to file rules, exemption
lists, warning rows, or their capacity settings. The finalizer calls them without exposing their private results to posting roles.

The rule resolver checks the current policy revision under its shared lock and
reads the board's work-safe setting. It considers at most 128 applicable rules,
tries the normalized digest first, and permits original-digest fallback only
when eligible. Overlapping matches fail rather than selecting an arbitrary row.
It returns the live reasons only to the private restriction owner.

A missing digest on an existing upload is unavailable evidence. It is not the
same as a request with no upload, which never needs this finalizer. Known
ineligibility skips original fallback; unknown eligibility matters if the
original digest matches. The database test compares 2,352 source cases with a
normalized digest. Six no-file source cases are excluded from this resolver
comparison; the resolver comparison alone does not qualify the no-file posting path.

The exemption bridge holds the shared policy lock and checks the exact transport
peer. It does not record a ban or alter a refusal response. The DMCA bridge uses
database time and records within its caller's transaction, so a failed finalizer
can roll back warning state with the other effects. Neither bridge replaces
upload-capability validation, staff authorization, or the actor and restriction
locks required by that finalizer.

## Posting finalizer

Migration 0192 adds `content.finalize_file_filter`. The Store checks exact-file
rules after content and network admission and before tripcode admission. It
skips this stage when there is no attachment or a trusted selected staff badge.
An upload's reservation capcode does not grant that posting exemption.

A candidate ends the ordinary posting transaction before starting a fresh
read-committed effect transaction. The finalizer locks the actor, restriction
gate, board and policy, then validates the upload capability and metadata again.
It requires the same rule generation and matched digest. Staff refusals also
revalidate the session, CSRF, rank and board scope. The actor hash and exact peer
come from the same verified transport identity; neither is a form field.

A rejected upload remains published and unconsumed. Quiet responses use the
source's fake post-number queries without creating a post. First DMCA attempts
record a warning; repeats within the inclusive day request a three-day global
ban and delayed banned-page response. An exemption suppresses ban persistence
but preserves the refusal or redirect. Optional anonymous-cookie refresh does
not register posting activity. The finalizer rechecks upload expiry and staff
authority after effect work, and failures roll back the transaction. An
uncertain commit is not automatically retried.

`restrictions.file_clock.timezone` starts as NULL. An operator must set a valid
database timezone to enable actual automatic-ban issuance. Reject-only rules,
first DMCA warnings and exempt attempts do not need that setting. Empty or
PHP-false public reasons use “Posting a blocked file.” This deliberate security
change keeps the private digest and filename audit out of the public reason.

HTML refusals use the existing banned page and a three-second Refresh header.
Enhanced posting keeps its JSON error envelope and adds the fixed
`x-board-posting-redirect: banned` signal. The client accepts it only alongside
the expected banned error and schedules the same-origin banned page after three
seconds. This is a response-adapter difference from the source's inline-script
HTML redirect, including when its caller requested JSON.

This integration does not complete issue #212. Historical template-226/6 counts,
remaining classifier providers and final caller-order coverage still need
qualification. File-rule provenance alone cannot supply those historical counts.
