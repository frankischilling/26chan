#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-request-authority.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-request-authority\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    if [[ $started = 1 ]]; then
        "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null 2>&1 || status=1
        if "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" status >/dev/null 2>&1; then server_status=0; else server_status=$?; fi
        if [[ $server_status != 3 ]]; then echo "Cluster stop unconfirmed; preserved $cluster" >&2; exit 1; fi
    fi
    rm -rf -- "$cluster"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
"${runner[@]}" "$pg_bin/initdb" -D "$cluster/data" --username=postgres --auth=trust --encoding=UTF8 --no-locale >/dev/null
started=1
# Keep the owned listener below the default Linux ephemeral-port range.
if ! "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15448 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null; then
    echo "Owned janitor authority database failed to start:" >&2
    [[ ! -f "$cluster/server.log" ]] || cat "$cluster/server.log" >&2
    exit 1
fi
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15448 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE request_authority OWNER board_migrator;
REVOKE ALL ON DATABASE request_authority FROM PUBLIC;
GRANT CONNECT ON DATABASE request_authority TO board_migrator,board_auth,board_staff;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth SET lock_timeout='2s';
ALTER ROLE board_auth SET statement_timeout='5s';
ALTER ROLE board_staff SET lock_timeout='2s';
ALTER ROLE board_staff SET statement_timeout='5s';
SQL
migrator=("${psql[@]}" -U board_migrator -d request_authority)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0206_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -f fixtures/demo.sql >/dev/null
"${migrator[@]}" -c BEGIN -f migrations/0206_janitor_request_authority.sql -c ROLLBACK
"${psql[@]}" -d request_authority <<'SQL'
DO $$ BEGIN
 IF to_regprocedure('post_secrets.janitor_request_unlocked(text,bigint)') IS NOT NULL
  OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request authority rollback changed the predecessor'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0206_janitor_request_authority.sql
"${psql[@]}" -d request_authority -f scripts/test-janitor-request-authority.sql
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15448 -U postgres -d request_authority --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE request_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15448 -U postgres -d request_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d request_restored <<'SQL'
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Restored request authority differs'; END IF; END $$;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF post_secrets.janitor_request_unlocked('demo',1000001) IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Restored unknown evidence unlocked'; END IF; END $$;
SQL
echo 'Janitor request authority upgrade, boundaries and restore passed.'
