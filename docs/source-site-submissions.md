# Feedback, applications, and contests

This work covers the remaining submission workflows in issue #225. Poll pages
and voting already have separate implementations. Feedback submission has a protected native form. Feedback review, janitor
applications, and banner contests are still being built; their domain models
and source-template checks do not make those routes functional.

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
cannot read submission rows; the configuration function owner currently has
access to profiles, its authority catalog, and the record identifiers/state
needed to check whether a submission action is open. The feedback writer can
insert feedback and read its bounded quota metadata, but cannot read feedback
message bodies or application contact fields. The catalog fingerprints
permissions and definitions, using names rather than transient database object
IDs so restoration can be checked without accepting fresh authority.

On an existing installation, bootstrap the NOLOGIN owner with
`deploy/site-submission-role.sql` before migration 0258. This migration leaves
all campaigns closed and adds no historical submissions.

Remaining implementation includes application and contest writes, ownership
checks, staff actions, upload processing, their native forms, and browser
qualification. No application automatically grants staff access. No contest
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
its separate Pass-only receipts are described below. Application and contest
forms still need their write workflows.


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
