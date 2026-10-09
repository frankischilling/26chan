#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-robot-cleanup.XXXXXXXX)
started=0
cleanup() {
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop > /dev/null
        started=0
    fi
    [[ $cluster =~ ^/tmp/board-robot-cleanup\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
    [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
    rm -rf -- "$cluster"
}
trap cleanup EXIT
chown postgres:postgres "$cluster"
runuser -u postgres -- "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale > /dev/null
runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$cluster'" -w start > /dev/null
started=1
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h "$cluster")
runuser -u postgres -- "${psql[@]}" -d postgres -f deploy/roles.sql
runuser -u postgres -- "${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE robot_cleanup_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE robot_cleanup_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE robot_cleanup_upgrade TO board_migrator,board_public,board_staff,board_auth,
    board_media,board_media_read,board_media_intake,board_monitor;
-- These runtime logins exist only in the private synthetic fixture.
ALTER ROLE board_staff LOGIN;
ALTER ROLE board_auth LOGIN;
ALTER ROLE board_media_read LOGIN;
ALTER ROLE board_media_intake LOGIN;
ALTER ROLE board_monitor LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d robot_cleanup_upgrade)
for migration in migrations/*.sql; do
    [[ $migration != migrations/0117_robot9000_cleanup.sql ]] || break
    "${migrator[@]}" --single-transaction -f "$migration"
done
"${migrator[@]}" <<'SQL'
INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at)
VALUES('r9k',decode(repeat('ab',32),'hex'),'2001-02-03Z');
INSERT INTO post_secrets.robot9000_mutes(board,actor,timeout_power,mute_until,next_expire)
VALUES('r9k',decode(repeat('cd',32),'hex'),3,'2020-01-01Z','2020-01-02Z');
CREATE TABLE public.owned_cleanup_rows_before AS
 SELECT 'texts' AS relation,to_jsonb(t) AS value FROM post_secrets.robot9000_texts t
 UNION ALL SELECT 'mutes',to_jsonb(m) FROM post_secrets.robot9000_mutes m
 UNION ALL SELECT 'boards',to_jsonb(b) FROM content.boards b;
CREATE VIEW public.owned_cleanup_rows_after AS
 SELECT 'texts' AS relation,to_jsonb(t) AS value FROM post_secrets.robot9000_texts t
 UNION ALL SELECT 'mutes',to_jsonb(m) FROM post_secrets.robot9000_mutes m
 UNION ALL SELECT 'boards',to_jsonb(b) FROM content.boards b;
CREATE TABLE public.owned_cleanup_functions AS
 SELECT oid,proowner,proacl::text AS acl,prosrc,proconfig FROM pg_proc
 WHERE pronamespace IN (SELECT oid FROM pg_namespace WHERE nspname IN('content','post_secrets','staff_identity'));
SQL
"${migrator[@]}" --single-transaction -f migrations/0117_robot9000_cleanup.sql
runuser -u postgres -- "${psql[@]}" -d robot_cleanup_upgrade <<'SQL'
DO $$ BEGIN
 IF EXISTS(TABLE public.owned_cleanup_rows_before EXCEPT TABLE public.owned_cleanup_rows_after)
 OR EXISTS(TABLE public.owned_cleanup_rows_after EXCEPT TABLE public.owned_cleanup_rows_before)
 OR EXISTS(SELECT * FROM public.owned_cleanup_functions EXCEPT
   SELECT oid,proowner,proacl::text,prosrc,proconfig FROM pg_proc)
 OR EXISTS(SELECT 1 FROM content.board_cleanup_audit) THEN
   RAISE EXCEPTION 'Cleanup upgrade rewrote history or existing functions';
 END IF;
END $$;
BEGIN;
SET LOCAL ROLE board_staff;
SELECT * FROM content.cleanup_robot9000('r9k',42);
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.robot9000_texts WHERE board='r9k')
 OR (SELECT count(*) FROM post_secrets.robot9000_mutes WHERE board='r9k')<>1
 OR NOT EXISTS(SELECT 1 FROM content.board_cleanup_audit WHERE board='r9k' AND account_id=42 AND removed=1) THEN
   RAISE EXCEPTION 'Cleanup lost its board-operation audit or changed mutes';
 END IF;
END $$;
ROLLBACK;
DO $$ BEGIN
 IF (SELECT count(*) FROM post_secrets.robot9000_texts WHERE board='r9k')<>1
 OR EXISTS(SELECT 1 FROM content.board_cleanup_audit) THEN
   RAISE EXCEPTION 'Cleanup rollback did not preserve history and audit';
 END IF;
END $$;
SQL
echo 'Robot9000 cleanup populated upgrade preserves history and existing functions; bounded cleanup and audit roll back together.'
