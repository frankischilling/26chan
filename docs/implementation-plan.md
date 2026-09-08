# Persisted public board implementation plan

The supplied mission defines the architecture. No separate attached design brief was available during inventory. Work uses the requested directory; the pre-existing nested checkout is excluded and unchanged. Public evidence is collected independently. Publication needs a confirmed rewrite remote.

The first slice uses a Rust domain/config/store workspace and a public Axum application. PostgreSQL persists boards, threads, posts, deletion credentials, and reports. Askama renders typed formatting nodes with escaped text. Media intake and staff routes remain unavailable until their boundaries and authentication exist.

- [x] Write and run failing domain/config tests; implement bounded formatting, board identifiers, posting validation, and origin validation.
- [x] Write database/HTTP tests; implement versioned schema, database roles, transactional posting and deletion, reports, HTML forms, JSON contracts and conditional responses.
- [x] Run concurrency and database denial tests against actual PostgreSQL logins. Test request limits, origin rejection, invalid deletion credentials and missing objects.
- [x] Add a synthetic dataset and browser checks. Record screenshot provenance without claiming reference parity.
- [x] Add CI, development setup, restricted service configuration, architecture/threat model, compatibility matrix, recovery exercise and precise evidence records.
- [x] Review the working checkpoint and prepare its verification record for the installed Git workflow helper.
- [ ] Push/open a draft PR once the rewrite remote is established. No publication target has been assumed.

Follow-on slices require independent evidence: isolated media jobs and promotion; WebAuthn staff identity and moderation; observed visual/behavior compatibility; deployed containment and recovery review. The first slice must not imply those are implemented.
