#!/usr/bin/env bash
# Explicit owned-runner qualification; no production activation or publication.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $# = 0 ]] || { echo 'Usage: test-paired-media-dispatch.sh' >&2; exit 2; }
[[ $(id -u) = 0 ]] || { echo 'Run on an owned disposable Linux host as root.' >&2; exit 1; }
[[ ${MEDIA_PAIRED_VM_QUALIFY:-} = 1 ]] || { echo 'Set MEDIA_PAIRED_VM_QUALIFY=1 explicitly.' >&2; exit 1; }
[[ ${APP_ENV:-} = development ]] || { echo 'Development qualification only.' >&2; exit 1; }
: "${MEDIA_VM_TEST_CONFIG:?Set the reviewed disposable decoder configuration}"
: "${MEDIA_VM_PROBE_CONFIG:?Set the separate containment-probe configuration}"
: "${MEDIA_PAIRED_COORDINATOR_TEST:?Set the compiled paired_vm integration-test executable}"
for path in "$MEDIA_VM_TEST_CONFIG" "$MEDIA_VM_PROBE_CONFIG" "$MEDIA_PAIRED_COORDINATOR_TEST"; do
  [[ $path = /* && -f $path && ! -L $path && -r $path ]] || { echo 'Expected absolute regular qualification artifacts.' >&2; exit 1; }
done
[[ -x $MEDIA_PAIRED_COORDINATOR_TEST ]] || { echo 'Coordinator test must be executable.' >&2; exit 1; }
for config in "$MEDIA_VM_TEST_CONFIG" "$MEDIA_VM_PROBE_CONFIG"; do
  [[ $(stat -c '%u:%a' "$config") = 0:600 ]] || { echo 'Qualification configs must be private root-owned files.' >&2; exit 1; }
done
# The same disposable-cluster identity check used by existing native dispatch.
# Verify before sourcing credential files or launching any VM.
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || exit 1
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Expected disposable database on port 55432.' >&2; exit 1; }
# Run decoder/containment qualification without credentials in its environment.
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin APP_ENV=development \
  MEDIA_PAIRED_VM_QUALIFY=1 MEDIA_VM_TEST_CONFIG="$MEDIA_VM_TEST_CONFIG" \
  MEDIA_VM_PROBE_CONFIG="$MEDIA_VM_PROBE_CONFIG" python3 tests/media/test_paired_vm.py -v
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/intake.env
: "${MIGRATION_DATABASE_URL:?Missing disposable migration credentials}"
: "${MEDIA_DATABASE_URL:?Missing disposable media credentials}"
: "${MEDIA_READ_DATABASE_URL:?Missing disposable media-reader credentials}"
: "${INTAKE_DATABASE_URL:?Missing disposable intake credentials}"
# Ignore and remove interpreter startup controls before the assert-based
# inherited harness is loaded. Direct Python entries also reject -O and -OO.
for variable in ${!PYTHON@}; do
  unset "$variable"
done
export APP_ENV=development
exec /usr/bin/python3 -E tests/media/test_paired_dispatch_vm.py
