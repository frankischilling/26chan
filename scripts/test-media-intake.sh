#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run on an owned disposable Linux host as root.' >&2; exit 1; }
: "${MEDIA_VM_TEST_CONFIG:?Set the disposable decoder configuration}"
: "${MEDIA_VM_PROBE_CONFIG:?Set the disposable boundary probe configuration}"
[[ $# = 0 || ( $# = 1 && $1 = --interrupt ) ]] || exit 2
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/intake.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
actual=$(runuser -u postgres -- /usr/lib/postgresql/16/bin/psql -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Expected disposable database on port 55432.' >&2; exit 1; }
exec python3 tests/media/test_intake_service.py "$@"
