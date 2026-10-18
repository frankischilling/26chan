# Report request profiles

Public report submissions capture the native HTTP profile and retain whether its
score reaches 0.4. This applies to both free-text and categorical reports. It does
not change report acceptance, queue priority or stored category weights.

The handler uses parser-ordered headers, configured origin and server request
time. Reporting always uses the model's OP flag, no multipart flag and no country
input, matching the reporting call's parameters. A dedicated opaque score type
rejects posting parameters. The caller remains responsible for supplying facts
from the same request; the store also checks the exact start time, canonical peer,
board and target post. It retains no raw headers, cookies or score total.

An unavailable capture or mismatched observation stays unknown. It does not
become a false predicate or a new report rejection. CAPTCHA, quotas, restrictions
and network policy keep their existing order and transaction boundaries.

## Evidence versions

Migration 0200 preserves all historical rows. Existing v1 rows retain a NULL
threshold. A new report with a qualified observation receives v2 and a non-NULL
boolean in the same transaction as its report, membership and activity. The
wrapper can update only the report it just inserted; there is no public evidence
setter. Failures roll back the whole admission.

Version 2 means the explicitly defined native HTTP profile. It is not proof of a
recovered historical PHP server environment. Consumers must not treat its false
value as evidence about an unobserved legacy request.

The pre-report known-or-verified observation still comes from the existing private
session helper, before registering the report. Staff authority, filtered-history
state and source reason remain NULL. The existing exact base-equals-fallback
weight proof is unchanged; this observation does not establish arbitrary numeric
weights or janitor, Pass or local-auth authority. Those remaining inputs prevent
a claim of complete reporting-weight parity.

## Checks

The catalog boundary seals the evidence constraints, wrappers, helper definitions
and grants. Public and staff runtimes cannot read or edit the private evidence.
Upgrade tests exercise rollback, hostile default grants, predecessor drift,
historical row preservation and populated dump/restore. Store tests cover both
threshold outcomes, unavailable and mismatched observations, and existing report
quotas, challenge consumption and rollback behavior. HTTP tests check the typed
reporting path against its ordinary reporting profile, including actual TCP
parser capture; domain tests check the inclusive 0.4 boundary.
