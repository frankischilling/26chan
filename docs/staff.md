# Staff authentication and moderation

The separate `board-staff` application enrolls and authenticates passkeys, displays reports, and applies audited moderation. Accounts and roles are provisioned with the offline `staff-operator` CLI. There is no password, recovery-code login, public bootstrap route, or runtime test authentication switch.

## Local operation

Use the disposable PostgreSQL cluster from `scripts/dev-db.sh`. Run `scripts/dev-staff-db.sh` as root in WSL, then apply versioned migrations through `board-migrate`. The provisioning script checks that port 55432 belongs to `.local/cluster-path` and refuses to overwrite `.local/staff.env` or `.local/staff.ps1`. These files contain separate random `board_auth` and `board_staff` login credentials and are ignored by Git.

Build the application, operator and synthetic browser helper:

```powershell
$env:OPENSSL_SRC_PERL = 'C:\Users\imike\4chan-rewrite\.local\strawberry-perl\perl\bin\perl.exe'
cargo build -p board-staff --bins --examples --locked
```

An operator process receives `MIGRATION_DATABASE_URL`. Invitations contain 256 random bits, expire after 30 minutes, and are stored only as SHA-256 hashes in PostgreSQL. Each output file must be in a **new** explicitly chosen parent directory. The CLI creates that directory with Unix mode 0700 or a Windows ACL limited to the current operator, then creates the file exclusively. It never prints the invitation and never overwrites an existing directory or file.

```powershell
. .local/database.ps1
target/debug/staff-operator.exe provision alice moderator .local/alice-enrollment/invitation.txt
target/debug/staff-operator.exe role alice admin
target/debug/staff-operator.exe recover alice .local/alice-recovery/invitation.txt
target/debug/staff-operator.exe revoke alice
```

Use a private channel to deliver enrollment material. Recovery deletes prior credentials, sessions, outstanding ceremonies and invitations before committing the replacement invitation. Revocation disables the account and deletes the same records. Role changes revoke sessions. Online staff cannot change their own roles.

Start the staff runtime in a fresh shell containing only its two runtime database credentials:

```powershell
. .local/staff.ps1
$env:STAFF_MODE = 'development'
$env:STAFF_BIND = '127.0.0.1:3001'
$env:STAFF_ORIGIN = 'http://localhost:3001'
$env:PUBLIC_ORIGIN = 'http://127.0.0.1:3000'
$env:MEDIA_ORIGIN = 'http://127.0.0.1:3002'
target/debug/board-staff.exe
```

Visit the exact configured staff origin. Paste the invitation into enrollment, approve the authenticator ceremony, then sign in with the provisioned account name. The report queue has handlers for close/reopen, sticky/unsticky, post/thread removal and report resolution/dismissal. Removed content remains visible to staff. Every mutation requires authentication within the preceding ten minutes; signing in again rotates the session and refreshes that interval. The queue displays up to 100 reports, with open reports first.

## Authentication and request boundaries

The application uses [webauthn-rs 0.5.5 passkey flows](https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/struct.Webauthn.html) and requires user verification. It stores ceremony state on the server in PostgreSQL for at most three minutes and deletes a challenge before verifying the finish request, including failed attempts. The serialization feature is used solely for protected server storage. It never sends serialized private ceremony state to the browser. Credential IDs are globally unique; login verifies and updates counters with an immutable-key, optimistic database update. A nonzero counter must advance against the current persisted value. Migration 0005 permits the library's legitimate one-way backup eligibility upgrade while rejecting a downgrade or replacement of key material.

Opaque sessions contain 256 random bits. PostgreSQL holds session and CSRF hashes, not raw cookie values. Every protected operation checks current account revocation, credential ownership, role and both absolute and inactivity expiry. Cookies are host-only, HttpOnly, SameSite=Strict and eight-hour lifetime; production adds Secure and the `__Host-` prefix. Mutations require the exact configured Origin, `Sec-Fetch-Site: same-origin`, and the session CSRF token. Authentication routes require the same Origin and Fetch Metadata checks before beginning a ceremony. Responses are private and no-store. CSP restricts scripts and forms to the staff origin. User text is rendered through the shared typed comment parser and escaped Askama templates.

`STAFF_IDLE_TIMEOUT_SECONDS` defaults to 900 (15 minutes). Integer values from 60 through 3600 are accepted; an empty, malformed or out-of-range value prevents startup. PostgreSQL checks the inactivity deadline and updates `last_activity_at` atomically using database time. A waiting request must still satisfy the deadline after a concurrent row update. Activity never extends the eight-hour absolute expiry or the ten-minute recent-authentication window, and does not issue a replacement cookie. An expired browser cookie may remain present, but protected requests receive HTTP 401 until a new WebAuthn login.

Opening the report queue or making a request that passes session authentication counts as activity. A later CSRF, object or recent-authentication denial can still occur. Login pages, scripts and readiness checks do not count, and there is no background keepalive. This measures authenticated server requests, not physical presence at the workstation. All staff instances must use the same timeout; changing it requires stopping staff serving and invalidating existing sessions before restarting. Increasing the timeout with old rows still present would otherwise make some previously idle rows eligible again. This policy follows the separation of idle and absolute expiry in [OWASP's session guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-expiration); the numerical limits are project decisions.

Limits are 32 KiB per request body, 16 admitted handlers and retained application response bodies/data, ten seconds per handler, eight connections per database identity, and two seconds for database pool acquisition. Each process allows 30 login/enrollment starts per minute. Each account has at most five outstanding ceremonies and ten sessions, enforced while holding an account advisory lock. Expired ceremonies are deleted on new starts; absolutely expired and idle sessions are deleted on login. Inaccessible expired rows otherwise remain until cleanup or operator revocation. Response data clones and slices retain the admission permit until released; dropping an unconsumed response also frees it. A proxy should cap connections and request/header/write duration. The handler deadline does not cover response delivery, and the admission count does not establish an aggregate memory limit. The in-process attempt budget resets on restart and is shared across staff users; multiple instances need a coordinated deployment limit.

`board_auth` reads identity records and operates ceremonies/sessions. It cannot create accounts, change roles, insert invitations, replace keys or directly insert credentials. A narrowly granted function enrolls only through a live invitation. `board_staff` reads content/reports, updates moderation columns, and appends audit records; it cannot read staff identities or alter/delete audit rows. Startup verifies actual login names and rejects privileged flags, role membership, database/schema ownership and cross-authority schema access. Public and media logins have no protected identity or audit access.

Moderation locks the board row used by public mutations, checks board/object relationships, and commits content changes, thread modification time and audit records in one transaction. A failed operation rolls back both content and audit changes.

The report queue includes escaped attachment filenames, byte sizes and dimensions. Nonspoiler images use bounded thumbnails from `MEDIA_ORIGIN`; spoilers require an explicit link. Removed, retired and expired files retain review metadata but cannot be reopened. The staff-only `content.staff_post_media` view exposes no job IDs, upload capabilities, processing leases or approval authority. Staff pages permit images only from the configured media origin, suppress image/link referrers and open downloads without a window opener. Staff and media must use different hostnames even in development; different ports do not isolate cookies.

`Remove file only` submits a normal POST form after WebAuthn login. It requires the same role, live session, ten-minute recent-authentication window, Origin/Fetch Metadata and CSRF checks as other moderation. It records `remove-file` with the account, board and post ID in the deletion transaction. It leaves the post text, thread and consumed-upload tombstone intact. Duplicate or unavailable removals return 404 without another audit entry. The reader denies full/thumbnail requests after removal, even with an old validator. Physical cleanup follows [the attachment retention policy](post-attachments.md#output-retention-and-cleanup).

Apply migration 0017 before starting this staff binary. It creates the display view and extends the audit action constraint without granting raw attachment/media access or adding a login. Readiness requires the view. No production service is activated by this migration.

## Native dependencies and verification

WebAuthn uses OpenSSL through the Rust bindings. This crate enables vendored OpenSSL; the lockfile currently selects OpenSSL source 3.6.3. Windows requires MSVC build tools and native Perl. The local run used [Strawberry Perl 5.42.2.1 portable](https://strawberryperl.com/release-notes/5.42.2.1-64bit.html), downloaded from the project's GitHub release into ignored `.local`, with SHA-256 `32d83be90cf04b807cfb9477482bc36302cdee6f5b04cf57e81adecbd8f07898`. Set `OPENSSL_SRC_PERL` per build command; no global Perl installation is needed. Linux vendored builds require a C compiler, make and Perl.

```powershell
. .local/database.ps1
. .local/staff.ps1
. .local/media.ps1
cargo test -p board-staff --features database-tests --locked
cargo clippy -p board-staff --all-targets --all-features --locked -- -D warnings
cargo build -p board-staff -p board-public -p board-media-http --bins --examples --locked
npx playwright test --config playwright.staff.config.js
```

The separate Playwright configuration starts staff and public applications with their own runtime credentials. Supply `MEDIA_READ_DATABASE_URL` to the test harness as well; it starts the actual reader on `127.0.0.1:3002` with only that database credential and a generated fixture store. The fixture executable requires migration authority and exists only under `examples/`. It inserts synthetic approval records and encodes fixed trusted pixels, bypassing upload/guest processing. Browser traces, screenshots and videos are disabled to avoid recording invitation/session material. The test uses a Chromium virtual authenticator and harmless disposable records. It checks enrollment/login, persisted moderation and audit entries, CSRF and stale-session denials, inactivity expiry with cookies retained, activity without recent-authentication renewal, logout, recovery, and public ETag invalidation. It also checks real thumbnail display, escaped filenames, spoiler non-fetching, absent media cookies/referrers, opener isolation, JavaScript-disabled file removal and full/thumbnail reader denial with old validators. Synthetic authentication does not demonstrate physical authenticator protection; this fixture is not decoder-containment evidence.

On September 13, 2026, the Windows PostgreSQL 16.15 staff suite passed all 28 tests, and the actual-reader Chromium scenario passed. The database test includes exact display-view columns/ACLs, unavailable credentials, expired/revoked/stale sessions, concurrent file removal and cancellation during a real audit-table lock. Cancellation leaves neither a tombstone nor an audit entry committed. Focused all-target/all-feature clippy passed with warnings denied. The first browser run reached successful deletion but failed when the harness inspected a request after its image tab closed; capturing headers when requests occur fixed the harness without weakening assertions. Populated restoration also passed again after migration 0017. Linux-specific shutdown tests remain in CI, not in the Windows count; the new staff head still needs CI. [Exact commands and evidence limits](verification-staff-attachments.md) are recorded separately.

## Migration 0006 and rollout

Stop every staff instance, apply migrations with the offline migration identity, then start the new staff binary and check `/readyz`. Migration 0006 adds activity timestamps and grants `board_auth` update access only to that column. Existing rows are initialized from `authenticated_at`, the only available activity evidence; sessions authenticated more than the configured idle period ago therefore require login again. Readiness checks require the new column. Public and media processes need no new privileges or configuration.

Do not run an older staff binary alongside the new one: it neither enforces nor updates inactivity. Rolling back the staff binary would remove the new control even if the column remains. Keep staff serving stopped if rollback is necessary until an approved version enforces the same policy. No down migration or production deployment is included. See [the verification record](verification-staff-idle.md) for the tested environment and remaining prerequisites.

## Production candidate

On Unix, SIGTERM and SIGINT stop application admission and allow active requests
to finish through Axum's graceful shutdown. The private metrics listener remains
available while requests drain and closes when serving ends. Other platforms keep
the Ctrl+C shutdown path. The existing handler timeout still applies; shutdown
does not establish a socket-write deadline or override an operator's forced-stop
deadline. SIGKILL cannot drain a request. [Native shutdown verification](verification-staff-shutdown.md)
records the real-binary regression and its execution evidence.

`deploy/staff.service` and `deploy/staff.env.example` are unactivated candidates. Production requires HTTPS at a local trusted reverse proxy, the exact origin configuration, separate runtime logins with reviewed grants, a private environment file, access-log redaction for authentication material, and tested operator delivery/recovery procedures. The service binds loopback and rejects unrelated database credentials. Each database URL must name its expected PostgreSQL login and production requires one canonical `sslmode=verify-full` option. Duplicate, alias and connection-identity override query options are rejected. Production public and staff hostnames must differ because cookies are not scoped by port; media must occupy a different registrable domain from both applications. Development uses loopback origins; the browser fixture separates public `127.0.0.1` from staff `localhost`. The candidate service limits memory to 512 MiB, CPU to one core, tasks to 64, file descriptors to 1024 and ordinary core dumps to zero. These deployment settings have not been activated or load-tested.

Attestation is **not enforced** in this slice. Passkeys may be synced or implemented in software. Hardware-backed credentials are preferred operationally, but the server does not establish hardware provenance. Physical authenticator validation, production identity provisioning, deployment and independent security review remain unverified.
