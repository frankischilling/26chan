#!/usr/bin/env bash
set -euo pipefail
cat >&2 <<'TEXT'
Public launch is blocked for this development milestone:
- No approved visual/behavioral reference snapshot or independent compatibility review.
- Media isolation, quarantine, promotion and deployed containment tests are absent.
- WebAuthn staff authentication, recovery and moderation workflows are absent.
- Production network identities, resource ceilings, backups and restore are unverified.
See docs/readiness.md. A passing unit test suite cannot override these prerequisites.
TEXT
exit 1
