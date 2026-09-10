#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 && $# = 2 ]] || { echo 'Run as root: test-maintenance-monitoring.sh ABS_OBSERVER_BIN ABS_TOOLS_DIR' >&2; exit 1; }
[[ $1 = /* && -x $1 && $2 = /* && -d $2 ]] || { echo 'Supply absolute existing observer binary and tools.' >&2; exit 1; }
python=${MAINTENANCE_QUALIFICATION_PYTHON:-/usr/bin/python3}
[[ $python = /* && -x $python ]] || { echo 'Supply an absolute existing qualification interpreter.' >&2; exit 1; }
options=()
case ${MAINTENANCE_QUALIFICATION_INTERRUPT:-0} in
  0) ;;
  1) options+=(--interrupt) ;;
  *) echo 'MAINTENANCE_QUALIFICATION_INTERRUPT must be absent, 0 or 1.' >&2; exit 1 ;;
esac
binary=$(readlink -f -- "$1")
tools=$(readlink -f -- "$2")
# Keep the venv interpreter pathname so its pinned dependency remains available.
exec env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LANG=C "$python" \
  tests/maintenance/interruption.py --binary "$binary" --tools "$tools" "${options[@]}"
