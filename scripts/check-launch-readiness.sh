#!/usr/bin/env bash
set -euo pipefail
cat >&2 <<'TEXT'
Public launch is blocked for this development milestone:
- No approved visual/behavioral reference snapshot or independent compatibility review.
- Local isolated media execution and operator recovery are tested; production host qualification, authenticated queue dispatch, publication crash reconciliation and fencing remain unverified.
- Staff WebAuthn and moderation are implemented locally; production authenticators, recovery policy and independent security review remain unverified.
- Production network identities, resource ceilings, backups and restore are unverified.
See docs/readiness.md. A passing unit test suite cannot override these prerequisites.
TEXT
exit 1
