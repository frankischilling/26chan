# ASVS evidence mapping

This is a partial implementation mapping to **OWASP ASVS 5.0.0**, tag [`v5.0.0_release`](https://github.com/OWASP/ASVS/tree/v5.0.0_release), commit `5cf9b032440be53ce345ab3c130fda46ba1ce7a2`. It is not an ASVS assessment or certification. Requirement identifiers below were checked in the tagged source; summaries are project-specific.

| ASVS requirement | Relevant implementation/evidence | Remaining scope |
|---|---|---|
| 1.2.1, 1.2.2 | Public and staff Askama escaping; typed HTTP(S) links and numeric quote paths; formatting and browser tests | Public media rendering remains disabled |
| 1.3.5, 1.3.7 | Bounded nonrecursive formatting grammar; templates are compiled developer files | Further parser/renderer review |
| 2.2.1, 2.2.2 | Server-side character/byte/range/identifier validation; SQL constraints; Unicode boundaries agree across forms, storage and rendering | Board-specific parity inventory and original Unicode counting unit incomplete |
| 2.3.2, 2.3.3, 2.3.4 | Documented limits, PostgreSQL transactions and shared board locks; public reply and media admission/claim contention tests | Broader mixed public/staff load and failure testing |
| 2.4.1 | Public peer limiter, staff authentication-start limiter, admission held through response data lifetime, body limits, bounded previews and media queue admission | Aggregate response-byte budgets, socket/write deadlines, distributed abuse control and external resource/load tests |
| 3.4.3, 3.4.4, 3.4.5, 3.4.6 | CSP, nosniff, same-origin referrer policy, denied framing on responses | Deployed proxy/header review |
| 3.5.1, 3.5.3 | Exact Origin/Fetch Metadata checks; mutations only on POST; staff session-bound CSRF and recent WebAuthn authentication; negative HTTP tests | Deployed proxy/browser review |
| 3.5.4 | Typed origin separation and separate registrable media domain | No deployed staff/media origins tested |
| 6.3.2, 6.3.4 | No default staff accounts or password/recovery-code login; operator provisions a selected role and expiring invitation | Production provisioning controls and recovery identity proofing |
| 6.4.1 | Random single-use enrollment invitations with expiry, consumed transactionally | Secure operator delivery and filesystem permissions in deployment |
| 6.5.6 | Operator revocation/recovery removes credentials and sessions; each request checks live account and credential state | Hardware token loss/recovery exercise and production support policy |
| 7.2.1, 7.2.2, 7.2.3, 7.2.4 | Server-validated opaque 256-bit random session tokens, hashed storage, rotation on authentication and prior session invalidation | Production session-store and cryptographic dependency review |
| 7.3.1, 7.3.2 | Database-enforced inactivity timeout and eight-hour absolute lifetime; requests cannot renew absolute expiry or WebAuthn authentication time | Production risk review of the configured timeout, coordinated configuration and rollout |
| 7.4.1, 7.4.2, 7.4.4 | Logout, live revocation check, visible logout control | Deployment and operator policy validation |
| 8.1.1, 8.1.2, 8.2.1, 8.2.2, 8.2.3 | Explicit moderator/admin roles, action allowlist, board/target relationship checks; runtime cannot administer roles | All-board moderation is deliberate; scoped board assignments are not implemented |
| 8.3.1 | Trusted staff handlers check live authentication before protected work; unavailable identity store fails closed | Cross-database revocation races and external service policy review |

Sources: [encoding/sanitization](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x10-V1-Encoding-and-Sanitization.md), [validation/business logic](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x11-V2-Validation-and-Business-Logic.md), [web frontend security](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x12-V3-Web-Frontend-Security.md), [authentication](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x15-V6-Authentication.md), [sessions](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x16-V7-Session-Management.md), [authorization](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x17-V8-Authorization.md).

This mapping identifies relevant implementation work; it does not mark any full ASVS chapter satisfied. In particular, hardware attestation, recovery identity proofing, deployed media containment and operations remain unverified or incomplete. [Staff inactivity verification](verification-staff-idle.md) adds server-enforced idle expiry without renewing absolute or recent-authentication deadlines; [the earlier verification](verification-media-staff.md) records the underlying authentication and privilege checks.
