#!/usr/bin/env bash
set -euo pipefail
cat >&2 <<'TEXT'
Public launch is blocked for this development milestone:
- No approved visual/behavioral reference snapshot or independent compatibility review.
- Local isolated execution, operator recovery and lease-fenced publication are tested; production host/storage qualification, authenticated dispatch and separate HTTP media serving remain unverified.
- Staff WebAuthn and moderation are implemented locally; production authenticators, recovery policy and independent security review remain unverified.
- Production network identities, resource ceilings, backups and restore are unverified.
See docs/readiness.md. A passing unit test suite cannot override these prerequisites.
TEXT
exit 1
