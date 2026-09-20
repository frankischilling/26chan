# Public request budgets

The public process loads these optional environment settings into
`board_config::PublicRequestLimits` before binding listeners or accessing the
database. Omitting a setting uses the documented default. A supplied
value must contain only ASCII decimal digits and be within its inclusive range;
empty values, signs, whitespace, overflow and zero fail startup. Errors name the
setting without printing its supplied value. Startup logs contain only the
validated numeric budgets, not the complete environment or database credentials.

| Setting | Default | Allowed range | Meaning |
| --- | --- | --- | --- |
| `PUBLIC_MAX_ACTIVE_REQUESTS` | 32 | 1-1024 | Shared public/JSON API admission slots |
| `PUBLIC_MAX_CONNECTIONS` | 128 | 1-4096 | Shared accepted public/JSON API connection slots |
| `PUBLIC_MAX_HASH_OPERATIONS` | 4 | 1-32 | Concurrent password-hashing slots |
| `PUBLIC_MAX_UPLOADS` | 4 | 1-32 | Concurrent upload slots |
| `PUBLIC_WRITES_PER_MINUTE` | 30 | 1-10000 | Writes admitted per tracked direct peer per 60-second window |
| `PUBLIC_MAX_TRACKED_PEERS` | 10000 | 1-100000 | Maximum live peer entries in the write limiter |
| `PUBLIC_HANDLER_TIMEOUT_MS` | 10000 | 1-120000 | Deadline for obtaining a response from the protected handler |
| `PUBLIC_HEADER_TIMEOUT_MS` | 10000 | 1-120000 | HTTP/1 request-header read deadline for accepted connections |
| `PUBLIC_CONNECTION_TIMEOUT_MS` | 120000 | 1-600000 | Absolute lifetime of each accepted connection |

The settings are independent and apply per process, not across replicas. Change
the service environment and restart to apply them. The public and optional
JSON-only listener share the same request and connection admission state;
enabling a second listener does not double either budget. Library router
constructors without explicit limits retain the defaults. The deployment example
lists the same defaults and contains no usable credentials.

## Enforcement and boundaries

Admission remains non-waiting. Exhausted admission, hashing, upload or peer-map
capacity returns an unavailable response rather than creating an unbounded
application queue. The admission lease remains attached to response bodies and
retained data frames; holding returned bytes can continue to occupy a slot after
the handler finishes. Lowering the budget deliberately increases the chance of
rejection. Higher values are not a claim that the host has sufficient resources.

Connection admission happens before HTTP parsing and is shared by the public and
JSON API listeners. Once all connection slots are occupied, a newly accepted
socket is closed immediately. The server does not create a queued connection
task or attempt to write an HTTP error on that excess socket.

Every admitted HTTP/1 connection has a request-header deadline. A peer that does
not finish a valid header block within `PUBLIC_HEADER_TIMEOUT_MS` is disconnected
before the router runs. The separate absolute connection lifetime starts when the
socket is accepted and covers silent peers, headers, bodies,
handlers, response writes and keep-alive time. It is not reset by later requests
on the same connection. Before that hard deadline, the server gracefully retires
the connection with a lead time equal to the smaller of the handler timeout and
one eighth of the connection lifetime. Retirement stops new keep-alive requests;
an exchange already in progress may finish during the remaining window. The hard
deadline still cancels anything that has not drained by then. With the defaults,
graceful retirement begins at 110 seconds and the hard cutoff remains 120 seconds.

Ordinary shutdown stops acceptance and drains owned connection tasks under their
original deadlines. Cancelling the serving future aborts those tasks. The
[candidate systemd unit](../deploy/public.service) retains `TimeoutStopSec=20`,
so the service manager can terminate the process before a connection's longer
application deadline. Qualify the service and proxy stop budgets together;
changing an application timeout does not change the service manager's budget.

The write limiter uses the canonical direct socket address in development or
the address supplied by the [kernel-authenticated Linux proxy](public-proxy.md).
`Forwarded` and `X-Forwarded-For` have no authority. Reaching a peer's write
allowance returns HTTP 429 with the existing `Retry-After: 60`; exhausting the
peer map returns HTTP 503. IPv4 and IPv4-mapped IPv6 share one bucket. These are
coarse abuse controls, not user authentication or a distributed denial-of-service
defense. Additional proxy hops require separate identity qualification.

The handler deadline returns HTTP 408 and drops the asynchronous handler future.
It does not forcibly stop blocking work already running. The connection deadline
can interrupt response delivery after a handler has committed a database write,
so clients must not automatically replay an ambiguous write. Hashing work retains
its own semaphore guard until it finishes. Request admission is released when its
response lease is dropped, not simply because the handler deadline fired.

These overrides do not change form/upload byte ceilings, database connection
limits, board content policy, queue limits, proxy trust, credential separation,
origin checks or media isolation. Production uploads remain disabled. Host and
edge resource limits remain necessary; the upper bounds are configuration
guardrails, not production sizing or load-test evidence. Complete production
qualification and reference compatibility remain unfinished. Local transport
limits do not qualify host, proxy or edge capacity and timeout behavior.

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

`apps/public/src/transport/tests.rs` uses real TCP sockets for connection
admission, disconnect recovery, silent and partial headers, absolute lifetimes,
keep-alive reuse and retirement, incomplete request bodies, blocked response
writes, listener cancellation, panic isolation and graceful draining. The
retirement regression keeps a request active across the soft cutoff, verifies its
response completes, then verifies the keep-alive socket closes before the hard
deadline. Header tests set the total connection lifetime beyond the outer test
deadline, so a total timeout cannot hide missing header enforcement. The response
test leaves a small client receive buffer unread, observes the producer stop
advancing, and serves a healthy control request before the blocked connection
expires.

`apps/public/src/main.rs` exercises the actual paired-listener entry point in
both admission directions and checks normal draining. Linux tests in
`apps/public/src/transport.rs` add Unix/TCP shared admission before complete
headers, kernel peer identity and owned socket cleanup.

On September 20, 2026, native Windows validation with Rust 1.94.0 passed
`cargo test -p board-config --locked --jobs 4` (21 tests) and
`cargo test -p board-public --locked --jobs 4 --quiet` (109 tests). Formatting and
Clippy for both packages with all targets and features also passed. These native
results do not include the Linux-only Unix/proxy cases or database-feature tests.
The CI workflow runs those on Ubuntu and retains the TCP/config/startup coverage
on Windows. The associated pull request records hosted results for its final commit.

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
