# Report network policy

This is part of the unfinished network-admission work in #212. Report POSTs
apply operator-owned range, ASN and signature rules inside the report
transaction. Opening the report form keeps the original advisory behavior:
GET checks the target and reporting quotas, but does not apply bans.

## Order and locking

The supplied source checks CAPTCHA, report quotas, explicit bans, then network
rules before saving the report (`imgboard.php:7365–7405`,
`modes/report.php:123–219`). The report wrapper supplies `is_report=true` without
OP or image flags (`lib/postfilter.php:875–882`). General and report-only rules
can match; OP-only and image-only rules cannot.

The database keeps the existing restriction gate, then takes the board lock,
network-generation shared lock, report gate and session lock. Network evaluation
runs after quotas and explicit reporting restrictions, before report insertion
or anonymous activity registration. The existing one-use challenge boundary
stays intact. A later refusal rolls back challenge consumption, membership,
report evidence and session changes together.

The policy-generation lock prevents operator edits from changing the decision
mid-transaction. The evaluator reads at most 129 scoped rules and refuses an
overflow before evaluating any rule. The private reader also covers staff-only
boards; the report path has no badge exemption.

## Request data and activity

The application resolves ASN from the same canonical transport peer before
opening the transaction. The transaction receives bounded derived comparison
values, bound to the board, target and report actor. Raw headers and browser-key
material are not included. The context is consumed once and never grants
CAPTCHA, staff, identity or Pass authority.

Rule expiry uses the middleware request-start clock. Leniency uses the locked,
request-adjusted pre-report activity clock. A private anonymous-owner helper
returns only the numeric fields needed for network leniency, including pending
post/report counts; it does not allocate or update a session. Missing providers
remain distinct from successful lookups with no match. A definite range or hint
match remains usable when the alternative provider is unavailable.

The staff-only legacy API also has a `ReportNetworkSource` entry point for
trusted transport/provider inputs. Compatibility entry points without those
inputs fail closed if a relevant configured rule needs that data. An empty policy does not require
providers. The public application supplies the context for both direct
free-text reports and legacy categorical reports.

## Database upgrade

Migration 0179 replaces the three report admission bodies while preserving their
ownership and entry-point grants. New helpers are callable only by their owner
and the report-admission owner. Runtime roles gain no private-table access.
Readiness checks validate the helper metadata and grants. The restriction
catalog seal includes the helper definitions and ACLs as well as the admission
callers; the migration checks the old seal before replacing and resealing it.

The populated upgrade fixture preserves reports, membership, grouping, weight
evidence, existing rules, boards, posts and threads. It also checks the
restriction, report-snapshot and thread-transfer readiness boundaries.

## Qualification and remaining work

The SQL report subset matches all 344 applicable cases in the independent
executed-source network corpus. The 51 report regression tests include quota
precedence, unchanged GET advice, refusal rollback, and both policy-generation
lock directions. The populated upgrade fixture and actual HTTP cases pass locally. The 98-case activity projection matrix, 15 provider/scope/clock/overflow
controls and the final regression repeat also pass. Normal and credit-based
CAPTCHA proofs remain usable after network refusal in both reporting modes.
Store, public and staff Clippy pass with warnings denied. These local runs use
the same no-TLS fixture manifests as the posting qualification; full production
and hosted checks remain required.

An initial HTTP fixture incorrectly submitted free text to the categorical
legacy route; it now uses each route's existing form contract. A second fixture
expected status 400 for a policy refusal, while the existing error mapping
returns 422. These fixture corrections did not change runtime error handling.

The first private-board test found that the policy owner's public-board RLS
filtered out rules through the reader's board join. The reader now receives
the locked board's work-safe value from the report owner, which already has
the necessary visibility and column grant. The policy reader stays private
and bounded; existing RLS and grants are unchanged. The focused test now
passes both unmatched acceptance and matched rejection.

Hosted qualification also found that the historical-erasure operator could no
longer run its report-readiness check: the new restriction-seal call needs its
owner, while the migrator has SET-only membership. The operator now assumes
that existing owner for the check and resets its role afterward. No grants were
expanded. The failure was reproduced locally; the corrected operator passes
the populated upgrade, bounded cleanup, private-board, stale-state, rollback,
fresh-install and dump/restore checks. Added controls retain the migrator's
direct-execution denial and reject drift in the report network helper.

Trusted Pass bypass and its source error footer remain unfinished. A public
cookie, staff badge or CAPTCHA exemption must not stand in for a qualified Pass
provider. The intentional IPv6 and unavailable-provider security differences
are described in [network policy](network-policy.md).
