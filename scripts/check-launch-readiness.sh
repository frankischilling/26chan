#!/usr/bin/env bash
set -euo pipefail
cat >&2 <<'TEXT'
Public launch is blocked for this development milestone:
- No approved visual/behavioral reference snapshot or independent compatibility review.
- Local authenticated dispatch, isolated execution, operator recovery, lease-fenced publication and separate read-only HTTP serving are tested; production identities/certificate operations, host/storage qualification and media-domain/network deployment remain unverified.
- Staff WebAuthn and moderation are implemented locally; production authenticators, recovery policy and independent security review remain unverified.
- Production network identities, resource ceilings, backups and restore are unverified.
See docs/readiness.md. A passing unit test suite cannot override these prerequisites.
TEXT
exit 1
