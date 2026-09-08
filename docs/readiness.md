# Readiness and remaining work

This milestone is a working local text-board foundation. The overall rewrite is incomplete and must not be publicly launched as production-ready. `scripts/check-launch-readiness.sh` intentionally fails and lists the missing prerequisites. It is not a mock containment test.

| Work item | Current state | Acceptance needed |
|---|---|---|
| Approved design brief | No separate attachment found | Resolve its location and check the implementation against it |
| Rewrite publication | Authorized target: [frankischilling/26chan](https://github.com/frankischilling/26chan); public foundation tracked in issue #1 | Review the feature branch and hosted checks before merging |
| Public text slice | Implemented and locally tested | Review compatibility exceptions and production abuse limits |
| Reference snapshot | API docs revision pinned; visual/behavioral source missing | Collect permitted desktop/mobile/reference states and establish exact supported clients/features |
| Media containment | No worker/intake/promoter exists; enablement rejected | Implement quarantine, narrow coordinator, isolated per-job guest, external quotas/network deny, cleanup, output protocol, idempotent promotion and actual deployed negative tests with healthy positive controls |
| Staff application | Reserved PostgreSQL grants only | Implement independent WebAuthn enrollment/login/revocation/recovery, host-only sessions, role checks, protected moderation, audit and safe previews; exercise missing/invalid/expired/revoked/unavailable state |
| Compatibility completion | Text-only JSON subset; one original visual theme | Board-specific evidence, archive/media interfaces, complete cache/DOM/CORS contracts, visual/error-state baselines |
| Production operations | Candidate public unit and disposable restore exercise | Deploy owned test identities/network policies, resource enforcement, metrics/alerts, backup deletion isolation, restore/RPO/RTO measurements, rollback and incident exercises |
| Independent review | One bounded static implementation review | Security review of deployed public/staff/media/maintenance boundaries before public launch |

These are concrete follow-up scopes, not GitHub issues that have already been opened. Media and staff boundaries cannot be marked satisfied by the current Rust or browser tests. A missing worker proves no worker access properties; it means that acceptance criterion is unverified.

The next implementation slice is quarantine/job state and a maintained isolated worker on an approved test processing tier, while keeping runtime enablement closed until its tests pass. Staff WebAuthn work can proceed separately once the intended staff origin, credential enrollment/recovery policy and operator provisioning environment are established. Original downloads, archive behavior and unspecified original posting rules require explicit compatibility decisions.
