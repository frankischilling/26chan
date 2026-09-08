# ASVS evidence mapping

This is a partial implementation mapping to **OWASP ASVS 5.0.0**, tag [`v5.0.0_release`](https://github.com/OWASP/ASVS/tree/v5.0.0_release), commit `5cf9b032440be53ce345ab3c130fda46ba1ce7a2`. It is not an ASVS assessment or certification. Requirement identifiers below were checked in the tagged source; summaries are project-specific.

| ASVS requirement | Relevant implementation/evidence | Remaining scope |
|---|---|---|
| 1.2.1, 1.2.2 | Contextual Askama escaping; typed HTTP(S) links and numeric quote paths; formatting and browser tests | Full staff/media contexts not implemented |
| 1.3.5, 1.3.7 | Bounded nonrecursive formatting grammar; templates are compiled developer files | Further parser/renderer review |
| 2.2.1, 2.2.2 | Server-side byte/range/identifier validation; SQL constraints | Board-specific parity inventory incomplete |
| 2.3.2, 2.3.3, 2.3.4 | Documented limits, PostgreSQL transactions and board locks; concurrent reply test | Additional thread/deletion/moderation contention tests |
| 2.4.1 | Socket-peer write limiter, concurrency/body limits, bounded previews | Distributed abuse control and external resource/load tests |
| 3.4.3, 3.4.4, 3.4.5, 3.4.6 | CSP, nosniff, same-origin referrer policy, denied framing on responses | Deployed proxy/header review |
| 3.5.1, 3.5.3 | Exact Origin/Fetch Metadata checks; mutations only on POST; negative HTTP tests | Staff CSRF/reauthentication not implemented |
| 3.5.4 | Typed origin separation and separate registrable media domain | No deployed staff/media origins tested |

Sources: [encoding/sanitization](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x10-V1-Encoding-and-Sanitization.md), [validation/business logic](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x11-V2-Validation-and-Business-Logic.md), [web frontend security](https://github.com/OWASP/ASVS/blob/v5.0.0_release/5.0/en/0x12-V3-Web-Frontend-Security.md). Authentication, sessions, authorization administration, media, operations and the remainder of the standard still require implementation-specific assessment.
