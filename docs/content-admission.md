# Configured content admission

Issue [#212](https://github.com/frankischilling/26chan/issues/212) remains open.
Public posting now evaluates operator-owned content rules before built-in
wordfilters and Robot9000 handling. Supported actions are OP autosage, private
logging, rejection, global automatic ban and quiet success without a post.
The supplied source has no private operator rule rows. The migration starts
with no configured rules; synthetic test policy is never imported as production
policy. Fixed subject, filename and public name/trip checks still apply with an
empty policy.

The source reference is `lib/postfilter.php:689-876` in revision
`545b7812d1849f7958d914950c91fdbbe38f6b22`. The extractor checks the complete
file hash before selecting the content-hook body and its pure normalization
helpers. It supplies synthetic database/session adapters and records every hit,
log and automatic-ban call. It never loads the application, RPC code, private
tables or executable operator extensions. Its 434 cases include 414 decision
and effect comparisons and 20 unavailable-query allow cases. The latter
describe a deliberate security difference: the Rust caller rejects unavailable
policy state instead of allowing the post.

## Matching and rule actions

Rules retain the first-match decision order, with explicit `position,id` order
replacing the source query's unspecified row order. Global rules have an empty
board; other rules name one board. Lowercase spoiler, code and SJIS markers are
removed only for matching/logging. Literal matching joins name, subject,
filename and comment through the [qualified ICU projection](admission-normalization.md).
Regexp matching uses separated name, subject and comment fields. OP autosage
uses its distinct title-cased projection and preserves ordinary-rule fallthrough.

The caller provides escaped display text and public trip markup after trip
hashing. Private trip input is neither matched nor stored in rule logs. The
caller checks raw bounds, cleaned required subjects, repeated-line spam and
line limits before configured rules. Rules then precede final blank-comment
and text-only subject admission, as in `imgboard.php:5733-5804`. A blank OP or
fileless reply can therefore receive a rule rejection or quiet response before
its ordinary missing-content error. With no matching rejection, the established
missing-content checks still apply.

The fixed public name/trip stage follows configured decisions and precedes final
markup admission, as in `imgboard.php:5748`. It retains the source's normalized
display-name check, exact legacy-trip comparison and fixed ASCII trip pattern,
including literal pipe characters in the source classes. The input is escaped
public display text and the derived legacy hash. A secure trip display leaves
the separate legacy value empty, as in the source. Raw trip passwords are never
passed to this stage. ASCII names use the same dot/case/character projection
without a native handle; non-ASCII names run within the existing worker limit.
Independent extraction records 352 name/trip cases, including both source
moderator decisions. Authenticated staff call-site exceptions remain unfinished.

The actual anonymous activity row is locked before known-user evaluation. Lenient
rules require the source's 24-hour known-or-verified decision and more than ten
posts; cookie age alone cannot confer that status. Autosage takes precedence
over logging and rejection. Logging proceeds to the fixed subject checks.
Rejection messages use the source strings, with the filter ID exposed only for
the test board.

The safe PCRE2 wrapper accepts paired or symmetric PHP delimiters, escapes and
the qualified `i,m,s,x,u,A,U,J,n,S` modifiers. It counts nonoverlapping matches,
including PHP's nonempty retry after an empty match. Unsupported modifiers,
invalid patterns and match exhaustion fail policy evaluation. No JIT or public
pattern input is permitted. The [native pattern specification](https://www.pcre.org/current/doc/html/pcre2pattern.html)
defines the lower-limit behavior used by the wrapper.

## Transactions and database permissions

Migration `0066_content_admission.sql` adds private rules, a policy generation,
capacity limits, hits, logs and bans. It changes no historical post, thread or
deletion proof. Posting locks board, policy generation, exact peer and anonymous
activity in that order. A rule update waits for active snapshots, and recording
an effect with an obsolete generation fails. Same-peer posts on different
boards serialize global ban decisions. Invalid, archived, closed or full reply
targets cannot create effects or fake success.

The restricted `board_admission_owner` has no login, staff/deployment access,
schema creation or direct post/thread authority. Public credentials can obtain
a scoped snapshot and request only the active rule's allowed action. They
cannot read or modify the private tables. A definer trigger stamps autosage
from the recorded rule proof; public credentials still cannot update the
thread's moderation flag. Upload filename reads lock the job, recheck its
one-use capability after waiting and require published/approved metadata.
Queued or revoked media cannot create filter effects. These functions expose
metadata only.

Explicit rule rejection commits its intended hit/ban, and quiet rejection
returns the ordinary success envelope without a post, thread, attachment,
deletion proof or consumed posting receipt. A log or autosage effect shares the
eventual posting transaction. Unexpected failures roll back the post and its
effects. Final missing-content or fixed name/trip rejection also rolls back a
preceding log or autosage hit. This deliberately avoids retaining incomplete posting effects;
the source's separate database writes can survive that later rejection.
Robot9000's intentional rejection retains preceding admission effects
while rolling back its post savepoint, matching the established caller order.

Hits suppress repeated rule/peer records for one rolling hour, across boards.
Unlike source `ip2long`, exact IPv6 peers remain distinct. Automatic bans apply
across public boards; negative one is permanent and positive lengths expire at
UTC midnight plus the configured days. The source server's ambient timezone is
not supplied, so UTC is explicit. These private automatic bans do not complete
the separate staff ban/status/appeal workflow in #220.

## Bounds and operation

Policy permits at most 128 applicable rules, 2,048 bytes per pattern and 1,024
match calls per evaluation. PCRE2 limits each match to 50,000 match operations,
depth 64 and 1,024 KiB native heap; operator directives cannot raise those
limits. [ICU bounds](admission-normalization.md) apply to combined fields.
At most two native evaluations run concurrently. Excess work is rejected;
cancellation retains the worker permit until native work finishes. These
counters do not provide a universal native deadline or allocation sandbox.

Private hits, logs and bans each default to 100,000 rows, with an operator
ceiling of one million per table. Capacity checks serialize on a separate row
and reject overflow without partial effects. Logs retain exact peers and
escaped public fields, so an operator must set retention and backup policy and
prune through migration/maintenance authority before capacity is exhausted.
Runtime log readers, a staff rule editor and automatic retention are not added
by this migration. Runtime credentials cannot change quotas or delete policy.

Bootstrap `deploy/roles.sql` on a fresh owned database. For an existing
deployment, create the new NOLOGIN owner and grant only SET membership to
`board_migrator` before running the additive migration. Apply maintained ICU
and PCRE2 packages, migrate, then restart public applications. Rollback to an
older application stops evaluating configured rules; the private tables remain.
Operators must therefore suspend public posting during an application rollback
when those rules are required. Do not edit an applied migration or its checksum.

## Qualification and remaining work

The recorded source decisions and effects match on Linux PCRE2 10.42 and
Windows's vendored 10.46. All 133 Windows domain tests pass. Ten focused Linux
database tests pass for action precedence, global ban expiry, known-user state,
cross-board posting, actual permission denials, trip secrecy, malformed policy,
SQL lock timeout, capacity rollback, generation locking, NULL-argument denials
and approved upload metadata. The fixed name/trip stage matches all 352 source
cases and actual public responses, including Unicode, earlier-rule precedence
and later effect rollback. The `phpé` proxy case preserves the source's byte
count rather than PostgreSQL's Unicode regexp count. Fresh restricted-role
bootstrap passes
all migrations, historical preservation and owner/runtime grant checks.
Workspace Clippy passes with warnings denied. The real HTTP case verifies
source error messages, early-validation versus final-content precedence,
quiet success with no saved post, display preferences
without trip secrets and unavailable-policy errors. All 481 client tests pass;
the BFCache fixture correction is recorded in [CI fixture verification](verification-ci-board-fixtures.md).
The complete workspace repeat passes all 674 tests, with no failures or ignored
tests. All 19 selected browser cases pass for anonymous ownership, script-free
posting, trips, wordfilters and randomizers across desktop/mobile layouts.
Current-head CI and the isolated upload qualification remain required.

The current caller bypasses authenticated staff;
the source `/test/` exception and anonymous staff posting are unfinished.
Full name preparation, generated fortune/dice scope, arbitrary regexp
compatibility, `D`/`X` modifiers, IP/ASN policy, normalized/original file
blacklists and optional classifier interfaces remain part of #212 and its
related issues. Missing private data is not invented or counted as passing
behavior. Browser and isolated upload regression checks remain required.

```text
php scripts/extract-content-admission-reference.php SOURCE crates/domain/tests/fixtures/content-admission.json --check
php scripts/extract-name-trip-admission-reference.php SOURCE crates/domain/tests/fixtures/name-trip-admission.json --check
cargo test -p board-domain content_admission --lib --locked
cargo test -p board-domain name_trip_admission --lib --locked
cargo test -p board-store -p board-public --test content_admission --all-features --locked
bash scripts/test-role-bootstrap.sh
```
