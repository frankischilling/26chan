#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${MIGRATION_DATABASE_URL:?Set MIGRATION_DATABASE_URL outside the public runtime}"
psql "$MIGRATION_DATABASE_URL" -X -v ON_ERROR_STOP=1 -f fixtures/demo.sql
