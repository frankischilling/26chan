# Feedback, applications, and contests

This work covers the remaining submission workflows in issue #225. Poll pages
and voting already have separate implementations. Feedback submission and protected staff review have native forms. Janitor
applications and banner contests are still being built; their domain models
and source-template checks do not make those routes functional. Feedback
search shortcuts, original client assets, and visual qualification remain open.

## What the supplied source establishes

The reference tree contains ten public/review templates for these workflows.
Their controllers, category seeds, application questions, field-limit constants,
and contest schedules are absent. The extractor pins each template's SHA-256,
replaces its external footer include with an empty owned footer, and renders it
using synthetic properties. It never runs the original bootstrap, fetches its
assets, executes its JavaScript, or loads historical submissions.

Run the checked-in source replay with PHP CLI installed:

```sh
python3 scripts/extract-site-submission-reference.py --check
```

To regenerate from the supplied reference tree:

```sh
python3 scripts/extract-site-submission-reference.py /path/to/reference
```

The 140 cases cover both site variants, feedback success/error/index pages,
application open/desktop/email/edit gates, review permissions, and contest
submission/listing states. The fixture contains the pinned templates so CI can
repeat the render without downloading another repository. Rust tests compare
active controls with the domain decisions; HTML comments are not controls.

Important details:

- Feedback has category, subject, and message fields, with no contact-email
  field. Its review reply form is commented out. Pending-row dismissal uses
  `can_dismiss`; draft approval and dismissal require the administrator view.
- Applications check whether submissions are open before checking desktop or
  edit authentication. Their initial review must hide name, handle, and email.
  The source exposes 26 timezone values from -12 through +13, one required board
  and an optional second choice. Its edit page preserves the selected timezone.
- Banner entries accept 468×60 JPEG, PNG, and GIF images. The visible rule says
  “less than 150KB”; the missing controller leaves the byte-unit interpretation
  unknown. The native model currently uses a strict 150 KiB limit.
- Vtuber entries accept JPEG/PNG up to 2048×2048. The native model interprets the
  visible 5MB rule as 5 MiB. These binary byte limits are reconstruction choices.
- Ordinary banner listings show scores outside voting. The Vtuber template
  hides the entire listing outside voting, making its score-column branch
  unreachable. Both variants hide scores while voting is available.
- Application success pages use a private edit link and disable referrers.
  Contest success pages describe a private status/deletion link. The templates
  do not establish the missing controllers' token generation or validation.

## Native boundaries

Operator-provided categories, boards, questions, and limits need explicit
validation. The synthetic fixture values are not production defaults.
Text remains untrusted text; browser `maxlength` is measured in UTF-16 units.
Contact addresses do not inherit the sign-in provider allowlist or alias ban.
Application identity and initial-review answers have separate Rust structures.

Private tables and bounded configuration readers are in place. Runtime roles
cannot read the private tables directly; the configuration function owner currently has
access to profiles, its authority catalog, and the record identifiers/state
needed to check whether a submission action is open. The feedback writer can
insert feedback and read its bounded quota metadata, but cannot read feedback
message bodies or application contact fields. A separate review owner can read
feedback after staff authorization; it cannot read application identity. The
catalog fingerprints
permissions and definitions, using names rather than transient database object
IDs so restoration can be checked without accepting fresh authority.

On an existing installation, bootstrap the NOLOGIN owner with
`deploy/site-submission-role.sql` before migration 0258. This migration leaves
all campaigns closed and adds no historical submissions.

Remaining implementation includes contest submission and withdrawal, private
image processing and review, and application onboarding after selection.
Public application forms and blind staff review are implemented; browser
qualification and the missing visual assets remain separate checks. No application automatically grants staff access. No contest
entry automatically earns a Pass. Missing original styles and artwork remain
separate visual-parity gaps.

The ownership model generates separate 256-bit random application and contest
capabilities. Storage uses a purpose-separated digest. Parsing only validates
encoding; it grants no access without a private record lookup and the remaining
workflow checks. Capabilities and contact structures have no debug/serialization
implementation. Native campaign dates use half-open server-time windows and a
bounded entry capacity; campaign configuration alone does not authorize an
upload or vote.

The storage checks run against actual restricted logins. They reject direct
submission-table reads and role switching, exercise configured/expired/closed
profiles, and detect extra column grants. The migration test checks rollback,
rejects pre-existing owner authority, retains prior board content, and restores
synthetic application records without resealing the catalog. PostgreSQL 16.15
passes these checks locally; CI repeats them before this work can merge.


## Submission verification

Migration 0259 adds six action scopes: feedback, application creation,
application authentication, application editing, contest submission, and contest
deletion. Each challenge binds the anonymous session, verified peer, action,
resource ID, and form nonce. These scopes have no board and cannot spend or grant
posting credits. A separate private consumer prevents them from authorizing
ordinary posts or reports. Each protected write must lock and recheck its profile or campaign before
consuming a proof in the same transaction.

The verifier checks that the configured workflow is open before issuing or
accepting an answer. Private consumption rechecks the workflow, session, context,
and expiry. Direct public consumption is denied. Local integration tests cover
six scope/target pairs, wrong context, cross-action use, closure, expiry,
transaction rollback, and one-use replay. The domain suite passes 345 tests,
and the original public readiness contract still passes with these scopes. Contest voting is excluded from CAPTCHA issuance;
its separate Pass-only receipts are described below. Public application forms now use the ownership APIs described below. Contest
submission forms still need their isolated media and ownership workflows.


## Contest membership receipts

Migration 0260 gives contest votes their own 30-second receipt ledger. Only the
isolated verifier can issue a receipt for an approved entry in an active voting
window. The consumer rechecks the anonymous session, live Pass session,
credential version, expiry, membership status, entry, campaign, peer, and form
context. A posting receipt or public Pass-year badge cannot substitute for it.
Voting does not extend or change a posting-network hold.

The private voter key binds the stable entitlement row and campaign. Changing a
device or invalidating and replacing credentials does not produce a new voter
identity for that campaign. This key is private bookkeeping, not a public
anonymity guarantee. The submission APIs do not return it to browsers.

Authorization gates use the current clock and are rechecked after proof locks.
Tests hold the membership-policy lock until a voting window expires, then
verify that consumption refuses the proof without burning it. The application
gate also crosses a deadline within one SQL statement. Restricted-role tests
cover wrong entries and contexts, direct public refusal, rollback, replay,
revocation, expired membership, and stable identity across owned device
sessions. Final vote storage, staff approval, media publication, and public
forms are still unfinished.


## Feedback writes and native form

Migration 0261 provides a narrow public write function. It locks the configured
profile, validates its category and text limits, checks capacity and a per-peer
cooldown, and consumes the feedback receipt in the insertion transaction. The
verified peer digest supplies the quota identity. Public callers cannot provide
a separate identity to evade that quota. The 60-second default cooldown and
bounded capacity are native safety controls, not recovered source constants.

The `/feedback` form keeps the source multipart encoding and category, subject,
message, verification, and submit controls. It requires an explicitly enabled
profile with operator-provided categories and limits. Without one, GET retains
the existing informational page and POST refuses submission. The migrator-only commands below configure these profiles.

The native form bounds the whole body to 128 KiB and rejects files, duplicate or
unknown fields, invalid UTF-8, wrong CSRF values, and mismatched profile IDs.
Verification binds the session, peer, profile, purpose, and form nonce. Pass
credentials do not bypass feedback verification. Submitted text stays literal
in private storage. Successful POST redirects to a read-only result page;
refreshing that page does not submit again. Error and success views use the
source template classes. The original stylesheet and script remain missing, so
this is not a claim of visual parity.

Restricted-role tests cover invalid categories without proof consumption,
replay, closure, cooldowns, and two submissions competing for the last slot. A
category removed while a writer waits forces a foreign-key failure after proof
consumption; the transaction restores both the receipt and the capacity count.
HTTP tests exercise the real isolated verifier, multipart submission, escaped
category labels, cross-origin and CSRF rejection, replay, and read-only success
refreshes. Browser execution and screenshots remain to be qualified.


### Configure feedback

Choose categories and limits for the installation. This example is synthetic;
it is not a recovered source configuration:

```json
{
  "version": 1,
  "subject_limit": 150,
  "message_limit": 2000,
  "capacity": 10000,
  "cooldown_seconds": 60,
  "categories": [
    {"id": 1, "label": "Example category", "short_label": "Example"}
  ]
}
```

```sh
cargo run -p board-store --bin board-submissions -- validate-feedback feedback.json
cargo run -p board-store --bin board-submissions -- open-feedback feedback.json
cargo run -p board-store --bin board-submissions -- close-feedback 1
```

Use the profile ID printed by `open-feedback` in the close command. Validation
is offline. Opening and closing require `MIGRATION_DATABASE_URL` with the actual
`board_migrator` login; assuming that role from another login is not sufficient.
An enabled profile must be explicitly closed before another is opened. Closing
retains submissions and categories. New profiles have new IDs, so forms for an
old profile cannot submit into its replacement. Concurrent opens admit exactly
one profile. At most 10,000 profiles can be retained through this tool.


## Protected feedback review

The staff listener serves `/feedback` separately from the public form. It has
the source pending, answered, dismissed, and draft filters, category navigation,
ID-cursor pagination, and a private detail page with the address, user agent,
and language. Each page contains at most 25 entries. Profile selection defaults
to the enabled profile, or the newest retained profile when feedback is closed.
Use `?profile=ID` to review an older profile. Category counts show pending
submissions. These paging and profile-selection rules are native choices where
the supplied controllers are absent.

Bootstrap `deploy/site-submission-review-role.sql` before migration 0262 on an
existing installation. The review owner has no login and no application-identity
access. The authentication service can call its scoped APIs, but cannot read
the tables directly. Public and content-service logins cannot call review APIs.
The catalog also fingerprints the delegated staff-session function.

Administrators can review feedback. Other staff need the explicit
`feedback_review` global flag; pending dismissal additionally needs
`feedback_dismiss`. Only administrators can approve or dismiss drafts. These
flags are native permission controls, not recovered source role assignments.
Actions require matching CSRF and recent authentication within ten minutes,
checked again after row-lock waits. A completed action changes only state and
update time and appends one private audit record. Repeated or competing actions
cannot add another transition. Feedback and reply text stay unchanged. The
source's commented-out reply form remains absent.

The APIs reject oversized inventories before constructing pages: at most
10,000 profiles and 100,000 feedback rows per profile. Upgrade checks preserve
existing feedback, reject pre-existing owner grants and authority drift, and
restore the catalog without resealing it. Tests cover same-account and
cross-account competing requests, insufficient permissions, wrong CSRF, stale
authentication, expiry during a lock wait, and extra identity grants. HTTP
tests cover escaped text, private responses, source action visibility, origin
checks, redirects, and staff readiness. Rendered controls match all 64 captured
review-template states. Required CI fixtures exercise public and staff forms
on desktop/mobile with scripts on and off; those browser runs have not been
executed in this local environment.

The review stylesheet is a native fallback. The original review CSS/client and
status icons have not been recovered. IP-search and ban-search shortcuts still
depend on the broader staff-search work; the current detail page does not link
to unfinished or external search services. Pass credentials are never stored
in feedback, and credential-based search links are intentionally omitted.
These pages do not establish complete source visual parity.

## Applicant-owned records

Migration 0263 adds application creation and owner authentication/editing in the
store. Bootstrap `deploy/application-owner-role.sql` first on an existing
installation. The public listener serves `/janitorapp` with these APIs.

The source form asks for contact email on initial submission. Its later email
prompt asks the holder of the private edit URL to repeat that address. It does
not establish mailbox ownership and does not send a verification link. The
native APIs use that matching-email rule with separate creation, authentication,
and editing CAPTCHA purposes. No mail is sent. A wrong email consumes the
attempt's proof, and authentication proofs cannot authorize edits.

Applicant identity remains in its separate private relation. The new no-login
owner supplies capability-bound operations to the public login. The public
login, feedback writer, and feedback reviewer cannot read the identity relation
directly. A valid random
application capability reveals only the IDs needed to request authentication.
Editable fields require the matching email and a fresh proof. Neither the raw
capability nor its digest implements debugging or serialization in Rust.

Creation checks the current campaign, its capacity, field limits, board choices,
and a peer-bound cooldown. The default cooldown is 60 seconds, configurable
between zero and one day. Identity, blind answers, counter, and proof consumption
commit together. One canonical email can own one application per campaign.
These limits are native abuse protections; the original submission controller
was not supplied. Full campaigns still permit existing applicants to authenticate
and edit until the campaign closes.

Edits preserve the ownership capability and review state, update both sets of
fields atomically, and require the current revision. Competing edits to one
revision admit one update. Changing the contact email changes the address needed
for later authentication. The accepted and signed states identified by the
source agreement code (2 and 4) refuse public edits. This native safeguard keeps
an accepted record from changing after staff selection. The original edit
controller's behavior in those states is unknown. No application operation
creates a staff account, accepts an agreement, or awards a Pass.

Store integration tests use the real verifier and owned CAPTCHA answers. They
cover literal private fields, malformed direct database input, wrong emails,
proof separation/replay, full-campaign editing, stale and competing revisions,
accepted/signed records, closure during a proof-lock wait, duplicate-email
rollback, and competing requests for the last capacity slot. Migration checks reject existing
owner authority and identity-grant drift, roll back cleanly, preserve prior
records, and verify a restored catalog without resealing it. Blind staff review, selection, and full visual qualification remain unfinished.

### Application campaign commands

Use explicit UTC Unix seconds for the half-open application window. No campaign
or question text is enabled by default. For example:

```json
{
  "version": 1,
  "starts_at": 1792800000,
  "ends_at": 1793404800,
  "boards": ["g"],
  "questions": ["First question", "Second question", "Third question", "Fourth question"],
  "field_limit": 150,
  "answer_limit": 2000,
  "capacity": 100,
  "cooldown_seconds": 60
}
```

```sh
cargo run -p board-store --bin board-submissions -- validate-applications applications.json
cargo run -p board-store --bin board-submissions -- open-applications applications.json
cargo run -p board-store --bin board-submissions -- close-applications 1
```

Choose the actual dates, boards, and questions before opening. Use the ID printed
by the open command when closing. Validation is offline; writes require the
actual migrator login. Closing retains applications and their identities. An
enabled campaign must be explicitly closed before another can open, including
when its date window has ended. At most 10,000 campaigns may be retained through
this tool. Concurrent opens admit one campaign. The exclusive campaign-table
lock also excludes row-lock acquisition, avoiding a counter-update lock cycle.

### Public application forms

`/janitorapp` follows the source closed, desktop-required, matching-email,
new-application, and edit states. The private capability appears in the success
page's edit URL and in the edit form, never in a database row as plaintext. The
success response is HTML after POST, as in the supplied template; repeating the
POST cannot create another record. The native route uses the source fixture's
`WEB_ROOT` because the original controller configuration is absent.

The form keeps the source field names, required fields, board selectors, four
questions, UTC-12 through UTC+13 options, and literal escaped values. Its
extracted timezone script uses the source's integer-hour truncation and leaves
an existing application's timezone alone. Forms work without that script.
When CAPTCHA issuance is rate-limited after authentication, the editable fields
remain visible. The native refresh button posts those fields back without
reading private answers or changing the record. A later submission still needs
the correct email, capability, revision, CSRF, and fresh editing proof.

The desktop check is a native user-agent classification for the visible gate,
not a security boundary. The original classification function and rejection
text were not supplied. Responses use private/no-store caching and strict-origin
referrers so private URL queries do not leak through navigation. The source
success page's no-referrer metadata remains. CSRF and same-origin checks,
a 768 KiB encoded-body limit, strict form decoding, and the accepted-record edit
restriction are native security differences. Script policy permits only the
fixed application script on the new/edit pages; uploads gain no script authority.

The form's inline CSS is preserved in a local stylesheet. Its referenced
YUI/global/generic sheets, logo, and timezone display-name map are absent. The
page shell uses a labeled native fallback and UTC offsets without invented city
names. These pages do not establish complete visual parity.

HTTP tests cover closed/mobile gates, escaped questions and applicant fields,
private-link authentication, CSRF/origin rejection, CAPTCHA cooldown recovery,
email changes, and replay. Control names/order and gates match all 32 captured
source-template states. Browser fixtures exercise desktop create/authenticate/
edit with scripts enabled and disabled, and both mobile rejection modes. They
run only in the explicit `RUN_APPLICATION_BROWSER=1` CI fixture; they have not
been executed locally.

## Contest schedules, approved listings, and votes

Migration 0264 adds protected vote storage and bounded listing readers. Bootstrap
`deploy/contest-owner-role.sql` first on an existing installation. The source
banner page explicitly supports accepting submissions for this week's boards
while last week's boards are voting. Several campaigns of one kind can therefore
remain enabled, including completed campaigns shown as previous events. The
old single-profile reader selects the newest enabled campaign deterministically;
new callers can read a campaign by ID or page through its schedule directory.

The native directory returns at most 16 campaigns per page. Approved-entry pages
contain at most 25 entries, use descending entry IDs, and accept bounded numeric
offsets. These page sizes and ordering are native choices because the original
controller was not supplied. Only approved entries appear. Scores are absent
while voting is open. Normal banners can show results after voting; the Vtuber
listing is hidden outside voting, matching its reachable source template. No
listing includes the contact email, credit name, ownership digest, peer identity,
or private voter key. Image intake, review/approval, delivery, owner withdrawal,
prize claims, and public contest pages are not wired to these readers yet.

A vote requires a purpose-bound Pass receipt from the verifier, a live anonymous
session and Pass session, the current credential version, an approved entry,
and an open campaign. The stable private voter key represents the Pass and
campaign, not a browser device, badge, or public purchase year. One Pass can vote
once per board in a campaign. Changing devices does not permit another vote.
Repeated verified votes return a duplicate result and consume that attempt's
proof. Closed windows, unavailable entries, exhausted capacity, and invalid
proofs cannot add a vote or change a score.

The campaign and entry locks remain held through proof consumption, ledger
insertion, and score/counter updates. The verifier checks the deadline again
after its own lock waits. These votes do not take or extend posting-network
holds. A separate no-login owner has only the contest columns and proof function
it needs; it cannot read contact emails or application identities. Extra contact
access invalidates the sealed submission catalog.

Tests use real owned Pass login/receipt flows. They cover two simultaneous votes
from one Pass, replay, a second device, a separate board, capacity refusal and
proof retention, private-field exclusion, score visibility, directory and entry
pagination, and Vtuber visibility. Upgrade tests preserve existing campaigns,
entries, votes, applications, and feedback; reject authority drift; roll back the
new functions and index change; and verify restored authority without resealing.

### Contest campaign commands

Configuration uses explicit UTC Unix seconds and half-open windows. For example:

```json
{
  "version": 1,
  "kind": "board_banner",
  "boards": ["g"],
  "submission_starts": 1792800000,
  "submission_ends": 1793404800,
  "voting_starts": 1793491200,
  "voting_ends": 1794096000,
  "capacity": 100,
  "vote_capacity": 1000
}
```

Use `"kind": "vtuber"` with `"boards": ["all"]` for the Vtuber scope. Choose the
actual dates and boards before opening a campaign.

```sh
cargo run -p board-store --bin board-submissions -- validate-contest contest.json
cargo run -p board-store --bin board-submissions -- open-contest contest.json
cargo run -p board-store --bin board-submissions -- close-contest 1
```

Validation is offline. Opening and closing require the actual migrator login.
Schedules for different boards can overlap. The operator rejects overlapping
submission-through-voting windows for the same kind and board; adjacent windows
are allowed. This is a native ambiguity guard, not a recovered scheduling rule.
Concurrent attempts to open the same schedule admit one campaign. Closing an
ID disables its readers and actions while retaining entries and votes. The
operator permits at most 10,000 retained campaigns. No campaign is opened by a
migration, and no prize or entitlement is issued by these commands.


## Blind application review

Migration 0265 adds staff-only application lists and details at /applications.
Administrators can review them; other staff need the explicit global
application_review flag. This is a native permission, not a recovered source
role assignment. Pages use 25-row ID cursors and retain closed campaign access.
They omit the separate name, handle, email, ownership hash, and actor hash.
Applicants can still identify themselves in their written answers; those answers
are displayed literally and HTML-escaped, not redacted.

Only a recently authenticated administrator can select or decline an applicant.
The decision must match the current revision and pending state. Row locks and a
second authentication check prevent competing decisions or an edit during review
from approving a stale record. One private audit row records the administrator,
revision, decision, and time. The native declined state is 1; selected state 2
matches the source agreement eligibility gate. No selection creates a staff
account, signs an agreement, or sends mail.

Selected contact fields require a separate request and recent administrator
authentication. The blind reviewer has no identity-table privileges. The
existing application owner supplies that narrow read for selected or signed
records only; it never returns ownership or actor hashes. All staff responses
remain private and non-cacheable.

The review layout is a native fallback because an application-review template
was not supplied. Native forms require no JavaScript. CI includes desktop/mobile
runs with scripts enabled and disabled, including selection, decline, reload,
and the separate contact view. Those browser checks cannot run in this local
environment. Agreement onboarding remains unfinished.
