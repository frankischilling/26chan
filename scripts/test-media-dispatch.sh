#!/usr/bin/env bash
# Owned native qualification; no shared credential provisioning or rotation.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run on an owned disposable Linux host as root.' >&2; exit 1; }
: "${MEDIA_VM_TEST_CONFIG:?Set the reviewed disposable decoder configuration}"
: "${MEDIA_VM_PROBE_CONFIG:?Set the separately reviewed boundary probe configuration}"
case "$*" in
  '') exercise=tests/media/test_dispatch.py ;;
  --systemd) [[ $# = 1 ]] || exit 2; exercise=tests/media/test_dispatch_services.py ;;
  --http) [[ $# = 1 ]] || exit 2; exercise=tests/media/test_http_service.py ;;
  *) echo 'Usage: test-media-dispatch.sh [--systemd|--http]' >&2; exit 2 ;;
esac
source .local/database.env
source .local/media.env
source .local/media-reader.env
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Expected disposable database on port 55432.' >&2; exit 1; }
exec python3 "$exercise"
