# Public request budgets

The public process loads these optional environment settings into
`board_config::PublicRequestLimits` before binding listeners or accessing the
database. Omitting a setting preserves the previous fixed value. A supplied
value must contain only ASCII decimal digits and be within its inclusive range;
empty values, signs, whitespace, overflow and zero fail startup. Errors name the
setting without printing its supplied value. Startup logs contain only the
validated numeric budgets, not the complete environment or database credentials.

| Setting | Default | Allowed range | Meaning |
| --- | --- | --- | --- |
| `PUBLIC_MAX_ACTIVE_REQUESTS` | 32 | 1-1024 | Shared public/JSON API admission slots |
| `PUBLIC_MAX_HASH_OPERATIONS` | 4 | 1-32 | Concurrent password-hashing slots |
| `PUBLIC_MAX_UPLOADS` | 4 | 1-32 | Concurrent upload slots |
| `PUBLIC_WRITES_PER_MINUTE` | 30 | 1-10000 | Writes admitted per tracked direct peer per 60-second window |
| `PUBLIC_MAX_TRACKED_PEERS` | 10000 | 1-100000 | Maximum live peer entries in the write limiter |
| `PUBLIC_HANDLER_TIMEOUT_MS` | 10000 | 1-120000 | Deadline for obtaining a response from the protected handler |

The settings are independent and apply per process, not across replicas. Change
the service environment and restart to apply them. The public and optional
JSON-only listener share the same admission state; enabling a second listener
does not double the budget. Library router constructors without explicit limits
retain the defaults. The deployment example lists the same defaults and contains
no usable credentials.

## Enforcement and boundaries

Admission remains non-waiting. Exhausted admission, hashing, upload or peer-map
capacity returns an unavailable response rather than creating an unbounded
application queue. The admission lease remains attached to response bodies and
retained data frames; holding returned bytes can continue to occupy a slot after
the handler finishes. Lowering the budget deliberately increases the chance of
rejection. Higher values are not a claim that the host has sufficient resources.

The write limiter uses the actual socket peer, not `Forwarded` or
`X-Forwarded-For`. Reaching a peer's write allowance returns HTTP 429 with the
existing `Retry-After: 60`; exhausting the peer map returns HTTP 503. Behind a
proxy, clients share the proxy's identity. Deployments requiring individual
client policies need a separately reviewed trusted-proxy design; merely sending
a forwarded header never grants one here. These are coarse abuse controls, not
user authentication or a distributed denial-of-service defense.

The handler deadline returns HTTP 408 and drops the asynchronous handler future.
It does not bound the lifetime of response streaming, configure socket/edge
timeouts, or forcibly stop blocking work already running. Hashing work retains
its own semaphore guard until it finishes. Admission is released when its
response lease is dropped, not simply because the handler deadline fired.

These overrides do not change form/upload byte ceilings, database connection
limits, board content policy, queue limits, proxy trust, credential separation,
origin checks or media isolation. Production uploads remain disabled. Host and
edge resource limits remain necessary; the upper bounds are configuration
guardrails, not production sizing or load-test evidence. Complete production
qualification and reference compatibility remain unfinished.

## Regression evidence

`crates/config/src/public_limits.rs` checks every default and each setting's
minimum, maximum, malformed values and out-of-range inputs. It also checks that
errors do not echo supplied marker values.

`apps/public/tests/startup.rs` invokes the real public binary with each invalid
setting and an occupied loopback listener. Invalid settings fail before binding
or database access without leaking marker credentials. A valid-configuration
control reaches the expected occupied-listener failure.

`apps/public/tests/http_limits.rs` uses non-default budgets against the real
middleware. It verifies shared admission in both public-to-API directions,
including retained response bytes; direct-peer write limits; peer-map capacity;
and ignored spoofed forwarded headers. Existing default-budget and streamed
request-size tests remain in place.

`apps/public/src/security.rs` tests configured hash/upload semaphore exhaustion
and release. A pending handler exercises the actual configured deadline,
handler cancellation and admission-lease release, with a separate outer test
deadline to prevent an unbounded test hang.

Run the targeted checks with an owned migrated PostgreSQL test database and the
repository's role-specific test environment:

```powershell
cargo fmt --all -- --check
cargo clippy -p board-config -p board-public --all-targets --all-features --locked --jobs 1 -- -D warnings
cargo test -p board-config --locked --jobs 1
cargo test -p board-public --all-features --locked --jobs 1 --quiet
npm run test:behavior
```

The full CI workflow additionally exercises the workspace, database permissions,
native media boundaries, restoration and pinned browser regressions on its
configured platforms. Local request-budget tests do not substitute for those
checks or for qualification on the deployment host.
