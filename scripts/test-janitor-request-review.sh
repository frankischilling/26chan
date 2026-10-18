#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-request-review.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-request-review\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=55450 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 55450 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE request_review OWNER board_migrator;
REVOKE ALL ON DATABASE request_review FROM PUBLIC;
GRANT CONNECT ON DATABASE request_review TO board_migrator,board_auth,board_staff;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth SET lock_timeout='2s';
ALTER ROLE board_auth SET statement_timeout='5s';
ALTER ROLE board_staff SET lock_timeout='2s';
ALTER ROLE board_staff SET statement_timeout='5s';
SQL
migrator=("${psql[@]}" -U board_migrator -d request_review)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0209_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -c BEGIN -f migrations/0209_janitor_request_review.sql -c ROLLBACK
"${psql[@]}" -d request_review <<'SQL'
DO $$ BEGIN IF to_regclass('restrictions.request_reviews') IS NOT NULL
 OR content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Request review rollback changed authority'; END IF; END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0209_janitor_request_review.sql
python3 scripts/ban-request-review-sql.py > "$cluster/source.sql"
"${psql[@]}" -d request_review -f "$cluster/source.sql"
"${psql[@]}" -d request_review -f scripts/test-janitor-request-store.sql
"${psql[@]}" -d request_review -f scripts/test-janitor-request-review.sql
request=$("${psql[@]}" -U board_staff -d request_review -Atc "SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207007,9003,false,false)")
[[ $request =~ ^[1-9][0-9]*$ ]] || exit 1
PGAPPNAME=owned_review_hold "${psql[@]}" -U board_staff -d request_review -v request="$request" > "$cluster/first-review.log" 2>&1 <<'SQL' &
BEGIN;
SELECT content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,:request,true,NULL,NULL,NULL,NULL,NULL);
SELECT pg_sleep(4);
COMMIT;
SQL
first=$!
ready=0
for attempt in $(seq 1 50); do
    if [[ $("${psql[@]}" -d request_review -Atc "SELECT count(*) FROM pg_stat_activity WHERE application_name='owned_review_hold' AND wait_event='PgSleep'") = 1 ]]; then ready=1; break; fi
    sleep 0.1
done
[[ $ready = 1 ]] || { cat "$cluster/first-review.log"; exit 1; }
if "${psql[@]}" -U board_staff -d request_review -v VERBOSITY=verbose -c "SELECT content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,$request,false,NULL,NULL,NULL,NULL,NULL)" > "$cluster/second-review.log" 2>&1; then
    echo 'Conflicting reviewer was accepted' >&2; exit 1
fi
grep -q '55P03' "$cluster/second-review.log" || { cat "$cluster/second-review.log"; exit 1; }
wait "$first" || { cat "$cluster/first-review.log"; exit 1; }
[[ $("${psql[@]}" -U board_staff -d request_review -Atc "SELECT content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,$request,false,NULL,NULL,NULL,NULL,NULL)->>'changed'") = false ]] || exit 1
"$pg_bin/pg_dump" -h 127.0.0.1 -p 55450 -U postgres -d request_review --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE review_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 55450 -U postgres -d review_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d review_restored <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true
  OR (SELECT count(*) FROM restrictions.request_reviews)<>3
  OR (SELECT count(*) FROM restrictions.request_ban_evidence)<>2
  OR NOT EXISTS(SELECT 1 FROM restrictions.request_totals WHERE approved=2 AND denied=1)
  OR (SELECT used_bytes FROM restrictions.capacity)<>(SELECT sum(charged_bytes) FROM restrictions.bans) THEN
  RAISE EXCEPTION 'Restored review evidence, counts or authority differs'; END IF;
END $$;
SQL
echo 'Janitor request review source, transaction and populated restore checks passed.' 
