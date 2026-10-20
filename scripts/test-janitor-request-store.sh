#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-request-store.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-request-store\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
if ! "${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15449 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null; then
    echo "Owned qualification database failed to start:" >&2
    [[ ! -f "$cluster/server.log" ]] || cat "$cluster/server.log" >&2
    exit 1
fi
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15449 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE request_store OWNER board_migrator;
REVOKE ALL ON DATABASE request_store FROM PUBLIC;
GRANT CONNECT ON DATABASE request_store TO board_migrator,board_auth,board_staff;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth SET lock_timeout='2s';
ALTER ROLE board_auth SET statement_timeout='5s';
ALTER ROLE board_staff SET lock_timeout='2s';
ALTER ROLE board_staff SET statement_timeout='5s';
SQL
migrator=("${psql[@]}" -U board_migrator -d request_store)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0207_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -c BEGIN -f migrations/0207_janitor_requests.sql -c ROLLBACK
"${psql[@]}" -d request_store <<'SQL'
DO $$ BEGIN
 IF to_regclass('restrictions.requests') IS NOT NULL
  OR content.restrictions_ready() IS DISTINCT FROM true
  OR content.report_snapshots_ready() IS DISTINCT FROM true
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Request storage rollback changed predecessor authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0207_janitor_requests.sql
"${migrator[@]}" -c BEGIN -f migrations/0208_janitor_request_form.sql -c ROLLBACK
"${psql[@]}" -d request_store <<'SQL'
DO $$ BEGIN IF to_regprocedure('staff_identity.janitor_request_form(bytea,bytea,integer,text,bigint)') IS NOT NULL
 OR content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request form rollback changed request authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0208_janitor_request_form.sql
if [[ ${RUN_RUST_TESTS:-0} = 1 ]]; then
    for migration in migrations/*.sql; do
        [[ $(basename "$migration") > 0208_janitor_request_form.sql ]] || continue
        "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
    done
    MIGRATION_DATABASE_URL=postgres://board_migrator@127.0.0.1:15449/request_store \
    AUTH_DATABASE_URL=postgres://board_auth@127.0.0.1:15449/request_store \
    STAFF_DATABASE_URL=postgres://board_staff@127.0.0.1:15449/request_store \
        cargo test -p board-staff --features database-tests --test janitor_requests --locked
fi
"${psql[@]}" -d request_store -f scripts/test-janitor-request-store.sql
# Two actual staff connections contend for one target. The second cannot
# create another request while the first transaction retains its actor lock.
PGAPPNAME=owned_request_hold "${psql[@]}" -U board_staff -d request_store > "$cluster/first.log" 2>&1 <<'SQL' &
BEGIN;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207005,9002,false,false);
SELECT pg_sleep(4);
COMMIT;
SQL
first=$!
ready=0
for attempt in $(seq 1 50); do
    if [[ $("${psql[@]}" -d request_store -Atc "SELECT count(*) FROM pg_stat_activity WHERE application_name='owned_request_hold' AND wait_event='PgSleep'") = 1 ]]; then ready=1; break; fi
    sleep 0.1
done
[[ $ready = 1 ]] || { cat "$cluster/first.log"; exit 1; }
"${psql[@]}" -U board_staff -d request_store <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207005,9002,false,false);
  RAISE EXCEPTION 'Conflicting request creator was accepted';
 EXCEPTION WHEN lock_not_available THEN NULL; END;
END $$;
SQL
wait "$first" || { cat "$cluster/first.log"; exit 1; }
"${psql[@]}" -d request_store <<'SQL'
DO $$ BEGIN IF (SELECT count(*) FROM restrictions.requests WHERE post_id=207005)<>1 THEN
 RAISE EXCEPTION 'Conflicting request creators changed outcome count'; END IF; END $$;
SQL
"$pg_bin/pg_dump" -h 127.0.0.1 -p 15449 -U postgres -d request_store --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE request_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 15449 -U postgres -d request_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d request_restored <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true
  OR content.report_snapshots_ready() IS DISTINCT FROM true
  OR content.text_thread_transfer_ready() IS DISTINCT FROM true
  OR (SELECT used_bytes FROM restrictions.request_capacity WHERE singleton) IS DISTINCT FROM
     (SELECT coalesce(sum(charged_bytes),0) FROM restrictions.requests) THEN
  RAISE EXCEPTION 'Restored request state or authority differs'; END IF;
END $$;
SQL
echo 'Janitor request capture, deletion, denial rollback and restore passed.'
