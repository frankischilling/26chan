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

Opaque sessions contain 256 random bits. PostgreSQL holds session and CSRF hashes, not raw cookie values. Every protected operation checks current account revocation, credential ownership, role and session expiry. Cookies are host-only, HttpOnly, SameSite=Strict and eight-hour lifetime; production adds Secure and the `__Host-` prefix. Mutations require the exact configured Origin, `Sec-Fetch-Site: same-origin`, and the session CSRF token. Authentication routes require the same Origin and Fetch Metadata checks before beginning a ceremony. Responses are private and no-store. CSP restricts scripts and forms to the staff origin. User text is rendered through the shared typed comment parser and escaped Askama templates.

Limits are 32 KiB per request body, 16 concurrent requests, ten seconds per handler, eight connections per database identity, and two seconds for database pool acquisition. Each process allows 30 login/enrollment starts per minute. Each account has at most five outstanding ceremonies and ten sessions, enforced while holding an account advisory lock. Expired ceremonies are deleted on new starts and expired sessions on login; inactive expired rows remain inaccessible until that cleanup or operator revocation. A proxy should also cap connections and request/header duration. The in-process attempt budget resets on restart and is shared across staff users; multiple instances need a coordinated deployment limit.

`board_auth` reads identity records and operates ceremonies/sessions. It cannot create accounts, change roles, insert invitations, replace keys or directly insert credentials. A narrowly granted function enrolls only through a live invitation. `board_staff` reads content/reports, updates moderation columns, and appends audit records; it cannot read staff identities or alter/delete audit rows. Startup verifies actual login names and rejects privileged flags, role membership, database/schema ownership and cross-authority schema access. Public and media logins have no protected identity or audit access.

Moderation locks the board row used by public mutations, checks board/object relationships, and commits content changes, thread modification time and audit records in one transaction. A failed operation rolls back both content and audit changes.

## Native dependencies and verification

WebAuthn uses OpenSSL through the Rust bindings. This crate enables vendored OpenSSL; the lockfile currently selects OpenSSL source 3.6.3. Windows requires MSVC build tools and native Perl. The local run used [Strawberry Perl 5.42.2.1 portable](https://strawberryperl.com/release-notes/5.42.2.1-64bit.html), downloaded from the project's GitHub release into ignored `.local`, with SHA-256 `32d83be90cf04b807cfb9477482bc36302cdee6f5b04cf57e81adecbd8f07898`. Set `OPENSSL_SRC_PERL` per build command; no global Perl installation is needed. Linux vendored builds require a C compiler, make and Perl.

```powershell
. .local/database.ps1
. .local/staff.ps1
. .local/media.ps1
cargo test -p board-staff --features database-tests --locked
cargo clippy -p board-staff --all-targets --all-features --locked -- -D warnings
cargo build -p board-staff --bins --examples --locked
npx playwright test --config playwright.staff.config.js
```

The separate Playwright configuration starts staff and public applications with their own runtime credentials. The fixture executable requires migration authority and exists only under `examples/`. Browser traces, screenshots and videos are disabled to avoid recording invitation/session material. The browser test uses a Chromium virtual authenticator and harmless disposable records. It checks enrollment/login, real persisted moderation and audit entries, CSRF and stale-session denials, logout, recovery, and public ETag invalidation. Synthetic authentication does not demonstrate physical authenticator protection.

## Production candidate

`deploy/staff.service` and `deploy/staff.env.example` are unactivated candidates. Production requires HTTPS at a local trusted reverse proxy, the exact origin configuration, separate runtime logins with reviewed grants, a private environment file, access-log redaction for authentication material, and tested operator delivery/recovery procedures. The service binds loopback and rejects unrelated database credentials. Each database URL must name its expected PostgreSQL login and production requires one canonical `sslmode=verify-full` option. Duplicate, alias and connection-identity override query options are rejected. Production public and staff hostnames must differ because cookies are not scoped by port; media must occupy a different registrable domain from both applications. Development uses loopback origins; the browser fixture separates public `127.0.0.1` from staff `localhost`. The candidate service limits memory to 512 MiB, CPU to one core, tasks to 64, file descriptors to 1024 and ordinary core dumps to zero. These deployment settings have not been activated or load-tested.

Attestation is **not enforced** in this slice. Passkeys may be synced or implemented in software. Hardware-backed credentials are preferred operationally, but the server does not establish hardware provenance. Physical authenticator validation, production identity provisioning, deployment and independent security review remain unverified.
