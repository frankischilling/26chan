#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable Linux host.' >&2; exit 1; }
[[ $# = 3 ]] || { echo 'Usage: test-queue-monitoring.sh ABS_MONITOR_BIN ABS_QUEUE_FIXTURE_BIN ABS_TOOLS_DIR' >&2; exit 1; }
[[ $1 = /* && $2 = /* && $3 = /* && -x $1 && -x $2 && -d $3 ]] || { echo 'Supply existing absolute binary and tool paths.' >&2; exit 1; }
BOARD_MONITOR_BIN=$(readlink -f -- "$1")
QUEUE_FIXTURE_BIN=$(readlink -f -- "$2")
MONITORING_BIN_DIR=$(readlink -f -- "$3")
pg_bin=/usr/lib/postgresql/16/bin
[[ -x $pg_bin/initdb && -x $pg_bin/pg_ctl ]] || { echo 'PostgreSQL 16 is required.' >&2; exit 1; }
cluster=$(mktemp -d /tmp/board-queue-monitor.XXXXXXXX)
owned_cluster=$(readlink -f -- "$cluster")
[[ $cluster = "$owned_cluster" && $cluster =~ ^/tmp/board-queue-monitor\.[[:alnum:]]+$ && ! -L $cluster ]] || exit 1
started=0
qualification_pid=
cleanup() {
  local result=$?
  trap - EXIT TERM INT
  if [[ -n $qualification_pid ]]; then
    # setsid below creates an owned process group; never kill by process name.
    kill -TERM -- "-$qualification_pid" 2>/dev/null || true
    for _ in {1..100}; do
      kill -0 -- "-$qualification_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL -- "-$qualification_pid" 2>/dev/null || true
    wait "$qualification_pid" 2>/dev/null || true
  fi
  [[ $cluster = "$owned_cluster" && $cluster =~ ^/tmp/board-queue-monitor\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster && $(readlink -f -- "$cluster") = "$owned_cluster" ]] || { echo 'Owned cluster path changed; refusing cleanup.' >&2; exit 1; }
  if [[ $started = 1 ]]; then
    [[ $(readlink -f -- "$cluster/data") = "$owned_cluster/data" ]] || { echo 'Owned data path changed; refusing stop.' >&2; exit 1; }
    if runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" status > /dev/null 2>&1; then
      runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m immediate -w -t 15 stop > /dev/null || { echo 'Owned PostgreSQL did not stop; refusing deletion.' >&2; exit 1; }
    elif [[ -e $cluster/data/postmaster.pid ]]; then
      echo 'PostgreSQL startup/exit is uncertain; refusing deletion.' >&2
      exit 1
    fi
  fi
  rm -rf -- "$owned_cluster"
  exit "$result"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth-local=peer --auth-host=scram-sha-256 --encoding=UTF8 --no-locale > /dev/null
port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')
started=1
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
  -o "-p $port -h 127.0.0.1 -k '$cluster'" -w -t 15 start > /dev/null
db=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster" -p "$port")
actual=$("${db[@]}" -At -d postgres -c 'SHOW data_directory')
[[ $actual = "$owned_cluster/data" ]] || { echo 'Started cluster identity differs.' >&2; exit 1; }
"${db[@]}" -d postgres -f deploy/roles.sql
BOARD_MONITOR_PASSWORD=$(openssl rand -hex 24)
BOARD_MEDIA_PASSWORD=$(openssl rand -hex 24)
BOARD_MIGRATION_PASSWORD=$(openssl rand -hex 24)
export BOARD_MONITOR_PASSWORD BOARD_MEDIA_PASSWORD BOARD_MIGRATION_PASSWORD
"${db[@]}" -d postgres <<'SQL'
\getenv monitor_password BOARD_MONITOR_PASSWORD
\getenv media_password BOARD_MEDIA_PASSWORD
\getenv migration_password BOARD_MIGRATION_PASSWORD
ALTER ROLE board_monitor LOGIN PASSWORD :'monitor_password';
ALTER ROLE board_media PASSWORD :'media_password';
ALTER ROLE board_migrator PASSWORD :'migration_password';
CREATE DATABASE board_queue_qualification OWNER board_migrator;
REVOKE ALL ON DATABASE board_queue_qualification FROM PUBLIC;
GRANT CONNECT ON DATABASE board_queue_qualification TO board_migrator, board_media, board_monitor;
SQL
for migration in migrations/*.sql; do
  "${db[@]}" -d board_queue_qualification --single-transaction -c 'SET ROLE board_migrator' -f "$migration"
done
MONITOR_DATABASE_URL="postgres://board_monitor:$BOARD_MONITOR_PASSWORD@127.0.0.1:$port/board_queue_qualification"
MEDIA_DATABASE_URL="postgres://board_media:$BOARD_MEDIA_PASSWORD@127.0.0.1:$port/board_queue_qualification"
MIGRATION_DATABASE_URL="postgres://board_migrator:$BOARD_MIGRATION_PASSWORD@127.0.0.1:$port/board_queue_qualification"
unset BOARD_MONITOR_PASSWORD BOARD_MEDIA_PASSWORD BOARD_MIGRATION_PASSWORD
QUEUE_QUALIFICATION=owned-disposable
# Keep Python's credentials/logs/storage inside the exact owned tree, including
# when a forced termination prevents Python's own TemporaryDirectory cleanup.
TMPDIR=$cluster
export BOARD_MONITOR_BIN QUEUE_FIXTURE_BIN MONITORING_BIN_DIR MONITOR_DATABASE_URL MEDIA_DATABASE_URL MIGRATION_DATABASE_URL QUEUE_QUALIFICATION TMPDIR
setsid python3 tests/monitoring/queue_qualify.py &
qualification_pid=$!
wait "$qualification_pid"
echo 'Queue qualification passed; removing its private PostgreSQL cluster.'
