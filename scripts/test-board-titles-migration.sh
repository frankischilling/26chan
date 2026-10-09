#!/usr/bin/env bash
# Owned disposable PostgreSQL only: populated 0119 -> 0120 with guarded short titles.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-titles.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-titles\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >/dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='' -c unix_socket_directories='$cluster' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f - < deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE title_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE title_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE title_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d title_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0120_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -f - < fixtures/demo.sql >/dev/null
"${migrator[@]}" -f - < scripts/board-titles-upgrade-before.sql
"${migrator[@]}" --single-transaction -f - < migrations/0120_board_short_titles.sql
"${migrator[@]}" -f - < scripts/board-titles-upgrade-after.sql
printf 'Title populated upgrade preserved operator edits, other board fields, content and authority.\n'
