# Watcher cleanup storage failure

Build 37574855191 on `2eb117f` passed the Linux Rust/database stages, then failed
one of 13 watcher/transport browser cases. The test body completed, but its first
cleanup GET returned 503 after the ordinary password-protected delete returned
303. The expected response remains 404.

The response was "Storage is unavailable. Try again later.", which identifies
the public application's generic store-error path. The retained trace does not
contain the underlying SQLx error. An aborted watcher request is also present,
but unwatch and cross-tab handlers cancel refreshes; this does not prove overlap
with deletion or explain the storage failure. Migration 0108 has not been shown
to cause it.

The existing private warning now classifies errors with a fixed vocabulary.
Known SQLSTATEs map to static labels; unknown codes remain `database_other`.
Database messages, SQL, bound values, identifiers and raw error codes are never
logged. Public status and body mappings remain unchanged. The owned Playwright
server pipes its existing output so future failures retain this classification.

Watcher fixture cleanup attempts every thread created by that test and reports
all failures afterward. It retains each real delete and 303/404 assertion.
There is no request retry, tab-close workaround or accepted failure baseline.

Four focused unit tests pass, including a hostile fake database message/code
that cannot appear in logs or the response. A fresh migrated database run passes
126 public library and two binary tests; formatting and strict workspace Clippy
also pass. These checks do not reproduce the hosted browser failure.

The last fully passing checkpoint before this failure was `1b6a231`, build
37570941094. A later successful run alone will not establish the cause or prove
that this intermittent failure is fixed.
