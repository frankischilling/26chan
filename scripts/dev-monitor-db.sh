#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run with sudo against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
[[ ! -e .local/monitor.env && ! -e .local/monitor.ps1 ]] || { echo 'Observer credentials already exist; reuse them.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster && ! -L $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
BOARD_MONITOR_PASSWORD=$(openssl rand -hex 24)
export BOARD_MONITOR_PASSWORD
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres <<'SQL'
\getenv monitor_password BOARD_MONITOR_PASSWORD
SELECT 'CREATE ROLE board_monitor NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'board_monitor')
\gexec
ALTER ROLE board_monitor LOGIN PASSWORD :'monitor_password';
ALTER ROLE board_monitor SET statement_timeout = '2s';
ALTER ROLE board_monitor SET lock_timeout = '1s';
ALTER ROLE board_monitor SET idle_in_transaction_session_timeout = '2s';
ALTER ROLE board_monitor SET search_path = pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_monitor;
SQL
umask 077
set -o noclobber
printf 'export MONITOR_DATABASE_URL=%q\n' "postgres://board_monitor:$BOARD_MONITOR_PASSWORD@127.0.0.1:55432/imageboard" > .local/monitor.env
printf '\044env:MONITOR_DATABASE_URL = '\''postgres://board_monitor:%s@127.0.0.1:55432/imageboard'\''\n' "$BOARD_MONITOR_PASSWORD" > .local/monitor.ps1
unset BOARD_MONITOR_PASSWORD
echo 'Independent observer credentials written to ignored .local/monitor.env and .local/monitor.ps1.'
