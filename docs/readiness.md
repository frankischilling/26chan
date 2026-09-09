# Readiness and remaining work

This milestone extends the working text board with private media plumbing and a separate staff application. The overall rewrite is incomplete and must not be publicly launched as production-ready. `scripts/check-launch-readiness.sh` intentionally fails and lists the missing prerequisites. It is not a mock containment test.

| Work item | Current state | Acceptance needed |
|---|---|---|
| Approved design brief | No separate attachment found | Resolve its location and check the implementation against it |
| Rewrite publication | Authorized target: [frankischilling/26chan](https://github.com/frankischilling/26chan); foundation merged in PR #2, media/staff tracked in issues #3/#4 | Review the feature branch and hosted checks before merging |
| Public text slice | Implemented and locally tested | Review compatibility exceptions and production abuse limits |
| Reference snapshot | API docs revision pinned; visual/behavioral source missing; [issue #6](https://github.com/frankischilling/26chan/issues/6) | Collect permitted desktop/mobile/reference states and establish exact supported clients/features |
| Media containment | Bounded private intake, queue leases/retries/cleanup and output promotion libraries tested; no worker, public enablement rejected; [issue #5](https://github.com/frankischilling/26chan/issues/5) | Integrate a narrow coordinator with an isolated per-job guest, external quotas/network deny, fenced publication/crash reconciliation and actual deployed negative tests with healthy positive controls; see [media notes](media.md) |
| Staff application | Separate WebAuthn app with real authentication/moderation logins, operator enrollment/recovery/revocation, expiring host-only sessions, safe previews and audited moderation | Hardware authenticators, attestation/recovery policy, inactivity timeout, deployed origins/network grants and independent review; see [staff notes](staff.md) and [actual verification](verification-media-staff.md) |
| Compatibility completion | Text-only JSON subset; one original visual theme | Board-specific evidence, archive/media interfaces, complete cache/DOM/CORS contracts, visual/error-state baselines |
| Production operations | Candidate public/staff units and disposable restore exercise | Deploy owned test identities/network policies, resource enforcement, metrics/alerts, backup deletion isolation, restore/RPO/RTO measurements, rollback and incident exercises |
| Independent review | Bounded implementation reviews recorded in verification notes | Security review of deployed public/staff/media/maintenance boundaries before public launch |

Issues [#3](https://github.com/frankischilling/26chan/issues/3) and [#4](https://github.com/frankischilling/26chan/issues/4) track the implemented media and staff slices. Issues #5 and #6 track the unavailable processing tier and reference work; the other rows identify remaining scope without implying that each has its own issue. Deployed media and staff boundaries cannot be marked satisfied by local Rust or browser tests. A missing worker proves no worker access properties; it means that acceptance criterion is unverified.

The media plumbing checkpoint implements quarantine/job state and bounded output conversion. Media enablement still requires a maintained isolated worker on an approved test processing tier and a production publication adapter. Staff WebAuthn work proceeds separately with an explicit origin and operator-controlled enrollment/recovery. Original downloads, archive behavior and unspecified original posting rules require explicit compatibility decisions.
