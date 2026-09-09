# Read-only API listener

The public process can bind a second listener for JSON clients. It serves `/boards.json`, `/{board}/thread/{id}.json`, `/{board}/threads.json`, `/{board}/catalog.json` and positive `/{board}/{page}.json` pages, plus `/healthz` and `/readyz`. Archives and attachments remain unavailable. The original JSON URLs on the HTML listener still work.

Set both `API_ORIGIN` and `API_BIND_ADDR` to enable it; omit both for the existing single-listener setup. In a fresh development shell, after the database is migrated and seeded:

```bash
source .local/database.env
unset MIGRATION_DATABASE_URL
API_ORIGIN=http://127.0.0.1:3003 API_BIND_ADDR=127.0.0.1:3003 cargo run -p board-public --locked
```

PowerShell equivalent:

```powershell
. .local/database.ps1
Remove-Item Env:MIGRATION_DATABASE_URL
$env:API_ORIGIN = 'http://127.0.0.1:3003'
$env:API_BIND_ADDR = '127.0.0.1:3003'
cargo run -p board-public --locked
```

The public process rejects staff, media and migration credentials. Use separate shells for those services. Test launchers remove unrelated credentials before starting the application.

## Origin and method contract

The pinned [API README](https://github.com/4chan/4chan-API/blob/2bd670d507ba2daa37a3961a661e088cf6f89d57/README.md) documents CORS from board origins for GET, HEAD and OPTIONS. This project maps that role to the exact configured `PUBLIC_ORIGIN`. It does not grant access to the original service's domains or arbitrary browser origins.

Browser clients use `credentials: 'omit'`. There is no wildcard origin or credentialed CORS grant. GET and HEAD responses expose `ETag` and `Last-Modified`, including conditional responses. Preflight requests may ask for `If-None-Match` and `If-Modified-Since`; other requested headers and write methods are rejected. Cache variation includes Origin even when a request supplies no Origin or an unapproved one. Explicit cache-header exposure, preflight errors and JSON error bodies on this listener are project-defined behavior, not verified replicas of undocumented responses.

The API has no posting, deletion, reporting, staff or HTML handlers. CORS controls browser readability; it is not authentication and does not stop non-browser clients from reading public JSON. The HTML listener retains its same-origin write checks. Public pages currently work without JavaScript and do not call the optional API; their CSP remains unchanged.

## Deployment and authority

In production, API_ORIGIN must be HTTPS with a registrable DNS domain, differ from the other origins, and use a different hostname from staff. Media must use a different registrable domain from the API as well as public and staff. Development origins and listeners are restricted to loopback. A duplicate listener address, invalid pair or occupied API port fails startup before database connection or serving requests.

Route the public hostname through the HTTPS reverse proxy to `BIND_ADDR`, and the API hostname only to `API_BIND_ADDR`. Apply TLS, header/connection limits and host allowlists at that proxy. Do not point the API hostname at the posting listener. This proxy configuration is an operator deployment prerequisite; no live proxy or TLS deployment was tested here.

Both listeners belong to one public process and share its database pool, 32-request concurrency budget and runtime credentials. Shutdown drains both and closes the shared pool. The API route surface is read-only, but the process still has the public application's posting authority. This is not a separate database privilege boundary. Staff identity, moderation and media processing remain separately deployed concerns.

This change needs no database migration. To remove API exposure, remove its proxy route and both API environment settings, then restart the public service through the operator's release procedure. Existing public HTML and JSON routes keep their URLs. Do not leave a proxy route pointing at a port that another service could later bind.

## Verification

`cargo test -p board-public --all-features --locked` exercises actual HTTP routers and PostgreSQL fixtures, including cache responses, forbidden routes, preflight denial and unavailable storage. `npm run test:behavior` starts the production binary with both listeners and runs cross-origin Chromium reads, conditional requests, denied-origin/credentialed reads and denied writes alongside existing posting tests.

The browser supplies a synthetic client document at an owned loopback origin; API responses are real network/database responses. Chromium's local-network permission is granted only to those test origins because intercepted documents lack a normal network address-space classification. CORS remains active. The denied-origin test first reads the same healthy API from the allowed origin. These checks establish local browser/API behavior, not deployed media containment, TLS policy or complete client parity.
