#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-message-upgrade.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-message-upgrade\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=55447 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 55447 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE message_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE message_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE message_upgrade TO board_migrator,board_auth,board_staff;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth SET lock_timeout='2s';
ALTER ROLE board_auth SET statement_timeout='5s';
ALTER ROLE board_staff SET lock_timeout='2s';
ALTER ROLE board_staff SET statement_timeout='5s';
SQL
migrator=("${psql[@]}" -U board_migrator -d message_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0205_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -f - < fixtures/demo.sql >/dev/null
"${psql[@]}" -d message_upgrade <<'SQL'
CREATE TABLE public.owned_message_upgrade_before AS
SELECT (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b) boards,
 (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM restrictions.templates t) templates;
SQL
"${migrator[@]}" -c BEGIN -f migrations/0205_staff_messages.sql -c ROLLBACK
"${psql[@]}" -d message_upgrade <<'SQL'
DO $$ BEGIN
 IF to_regclass('restrictions.staff_messages') IS NOT NULL
  OR to_regprocedure('staff_identity.staff_message_page(bytea,bytea,integer,integer,bigint)') IS NOT NULL
  OR content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Staff message rollback changed the predecessor'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0205_staff_messages.sql
"${psql[@]}" -d message_upgrade <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.staff_messages)
  OR (SELECT boards FROM public.owned_message_upgrade_before) IS DISTINCT FROM
     (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b)
  OR (SELECT templates FROM public.owned_message_upgrade_before) IS DISTINCT FROM
     (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM restrictions.templates t) THEN
  RAISE EXCEPTION 'Message upgrade changed retained configuration or invented messages'; END IF;
END $$;
SQL
if [[ ${RUN_RUST_TESTS:-0} = 1 ]]; then
    MIGRATION_DATABASE_URL=postgres://board_migrator@127.0.0.1:55447/message_upgrade \
    AUTH_DATABASE_URL=postgres://board_auth@127.0.0.1:55447/message_upgrade \
    STAFF_DATABASE_URL=postgres://board_staff@127.0.0.1:55447/message_upgrade \
        cargo test -p board-staff --features database-tests --test messages --locked
fi

"${psql[@]}" -d message_upgrade -f scripts/test-staff-messages.sql
"${psql[@]}" -U board_auth -d message_upgrade -f scripts/test-staff-messages-runtime.sql
"$pg_bin/pg_dump" -h 127.0.0.1 -p 55447 -U postgres -d message_upgrade --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE message_restored OWNER board_migrator;
SQL
"$pg_bin/pg_restore" -h 127.0.0.1 -p 55447 -U postgres -d message_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d message_restored -c 'GRANT CONNECT ON DATABASE message_restored TO board_auth'
"${psql[@]}" -U board_auth -d message_restored -f scripts/test-staff-messages-runtime.sql
"${psql[@]}" -d message_upgrade <<'SQL'
BEGIN;
GRANT SELECT(content) ON restrictions.staff_messages TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private message grant drift was accepted'; END IF; END $$;
ROLLBACK;
UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE username='owned-message-janitor';
SQL
"${psql[@]}" -U board_auth -d message_upgrade <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM staff_identity.staff_message_page(decode(repeat('11',32),'hex'),decode(repeat('33',32),'hex'),600,0,0);
  RAISE EXCEPTION 'Revoked message account accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
SQL
printf 'Staff message rollback, visibility, authority, grant drift and populated restore passed.\n'
