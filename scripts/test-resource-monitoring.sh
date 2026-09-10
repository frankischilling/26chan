#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 && $# = 2 ]] || { echo 'Run as root: test-resource-monitoring.sh ABS_RESOURCE_MONITOR_BIN ABS_TOOLS_DIR' >&2; exit 1; }
[[ $1 = /* && -x $1 && $2 = /* && -d $2 ]] || { echo 'Supply absolute existing resource binary and tool paths.' >&2; exit 1; }
python=${RESOURCE_QUALIFICATION_PYTHON:-/usr/bin/python3}
[[ $python = /* && -x $python ]] || { echo 'Supply an absolute existing qualification Python interpreter.' >&2; exit 1; }
options=()
case ${RESOURCE_QUALIFICATION_INTERRUPT:-0} in
  0) ;;
  1) options+=(--interrupt) ;;
  *) echo 'RESOURCE_QUALIFICATION_INTERRUPT must be absent, 0 or 1.' >&2; exit 1 ;;
esac
binary=$(readlink -f -- "$1")
tools=$(readlink -f -- "$2")
# Preserve the venv interpreter pathname; resolving its Python symlink would
# discard the hash-pinned bcrypt environment. No caller credentials are passed.
exec env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LANG=C "$python" \
  tests/monitoring/resource_interruption.py --binary "$binary" --tools "$tools" "${options[@]}"
