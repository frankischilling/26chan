#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
[[ -f .local/database.env && -f .local/cluster-path ]] || { echo 'Run scripts/dev-db.sh first.' >&2; exit 1; }
[[ ! -e .local/staff.env && ! -e .local/staff.ps1 ]] || { echo 'Staff credentials already exist; reuse them.' >&2; exit 1; }
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
actual=$(runuser -u postgres -- "$pg_bin/psql" -XAt -h /tmp -p 55432 -d postgres -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
auth_password=$(openssl rand -hex 24)
staff_password=$(openssl rand -hex 24)
runuser -u postgres -- "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres -v auth_password="$auth_password" -v staff_password="$staff_password" <<'SQL'
ALTER ROLE board_auth LOGIN PASSWORD :'auth_password';
ALTER ROLE board_staff LOGIN PASSWORD :'staff_password';
ALTER ROLE board_auth SET statement_timeout = '5s';
ALTER ROLE board_auth SET lock_timeout = '2s';
ALTER ROLE board_auth SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_auth SET search_path = pg_catalog;
ALTER ROLE board_staff SET statement_timeout = '5s';
ALTER ROLE board_staff SET lock_timeout = '2s';
ALTER ROLE board_staff SET idle_in_transaction_session_timeout = '5s';
ALTER ROLE board_staff SET search_path = pg_catalog;
GRANT CONNECT ON DATABASE imageboard TO board_auth, board_staff;
SQL
umask 077
set -o noclobber
printf 'export AUTH_DATABASE_URL=%q\nexport STAFF_DATABASE_URL=%q\n' "postgres://board_auth:$auth_password@127.0.0.1:55432/imageboard" "postgres://board_staff:$staff_password@127.0.0.1:55432/imageboard" > .local/staff.env
printf '\044env:AUTH_DATABASE_URL = '\''postgres://board_auth:%s@127.0.0.1:55432/imageboard'\''\n\044env:STAFF_DATABASE_URL = '\''postgres://board_staff:%s@127.0.0.1:55432/imageboard'\''\n' "$auth_password" "$staff_password" > .local/staff.ps1
echo 'Separate development credentials written to ignored .local/staff.env and .local/staff.ps1.'
