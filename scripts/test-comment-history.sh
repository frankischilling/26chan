#!/usr/bin/env bash
# Owned disposable cluster; no application configuration or production data.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-comment-history.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-comment-history\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=15458 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null || { cat "$cluster/server.log" >&2; exit 1; }
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 15458 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE comment_history OWNER board_migrator;
REVOKE ALL ON DATABASE comment_history FROM PUBLIC;
GRANT CONNECT ON DATABASE comment_history TO board_migrator,board_verifier,board_public,board_moderation_retention;
ALTER ROLE board_moderation_retention LOGIN;
SQL
migrator=("${psql[@]}" -U board_migrator -d comment_history)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0243_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('owneddupe','Owned expiry','Synthetic',2000,1000,1000,1000,10);
INSERT INTO content.threads(id,board) VALUES(800001,'owneddupe');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 VALUES(800001,'owneddupe',800001,'Anonymous','Owned history','Unknown old projection');
SQL
"${migrator[@]}" -c BEGIN -f migrations/0243_source_comment_duplicates.sql -c ROLLBACK >/dev/null
"${psql[@]}" -d comment_history <<'SQL'
DO $$ BEGIN
 IF to_regclass('post_secrets.comment_history') IS NOT NULL OR content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Rollback changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" --single-transaction -f migrations/0243_source_comment_duplicates.sql >/dev/null
"${psql[@]}" -d comment_history <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM post_secrets.comment_history) OR content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Migration invented old comparison data or changed authority'; END IF;
END $$;
SQL
"${migrator[@]}" <<'SQL'
-- Explicit synthetic operator setup; no claim to recover historical projection.
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
 SELECT n,'owneddupe',800001,'Anonymous','','Owned expiry row',clock_timestamp()-interval '2 hours' FROM generate_series(800002,801005) n;
INSERT INTO post_secrets.comment_history(post_id,board,actor_hash,projection_hash,projection_version,request_at,captured_at)
 SELECT n,'owneddupe',decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'),1,
 floor(extract(epoch FROM clock_timestamp()-interval '2 hours'))::bigint,clock_timestamp()-interval '2 hours'
 FROM generate_series(800002,801005) n;
UPDATE post_secrets.comment_history SET captured_at=clock_timestamp() WHERE post_id=801005;
UPDATE post_secrets.comment_history SET request_at=floor(extract(epoch FROM clock_timestamp()))::bigint WHERE post_id=801004;
SQL
"${psql[@]}" -U board_public -d comment_history <<'SQL'
DO $$ BEGIN
 BEGIN PERFORM 1 FROM post_secrets.comment_history; RAISE EXCEPTION 'Public can read history' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM content.expire_comment_history(); RAISE EXCEPTION 'Public can expire history' USING ERRCODE='ZX001';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
count=$("${psql[@]}" -U board_moderation_retention -d comment_history -Atc 'SELECT content.expire_comment_history()')
[[ $count = 1000 ]]
MODERATION_RETENTION_DATABASE_URL=postgres://board_moderation_retention@127.0.0.1:15458/comment_history \
 target/debug/board-comment-history-retention run | grep -q '"expired_comment_history_rows":2'
if MODERATION_RETENTION_DATABASE_URL=postgres://board_migrator@127.0.0.1:15458/comment_history \
 target/debug/board-comment-history-retention run > "$cluster/wrong-role.log" 2>&1; then
    echo 'Retention accepted the migration login' >&2
    exit 1
fi
"${psql[@]}" -d comment_history <<'SQL'
DO $$ BEGIN
 IF (SELECT count(*) FROM content.posts WHERE board='owneddupe')<>1005
 OR (SELECT count(*) FROM post_secrets.comment_history)<>2
 OR content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Expiry removed content or recent history'; END IF;
END $$;
SQL
"${runner[@]}" "$pg_bin/pg_dump" -h 127.0.0.1 -p 15458 -U postgres -d comment_history > "$cluster/restore.sql"
"${psql[@]}" -d postgres -c 'CREATE DATABASE comment_history_restore OWNER board_migrator'
"${psql[@]}" -d comment_history_restore -f "$cluster/restore.sql" >/dev/null
"${psql[@]}" -d comment_history_restore <<'SQL'
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true OR (SELECT count(*) FROM post_secrets.comment_history)<>2 THEN
 RAISE EXCEPTION 'Restore changed comment authority'; END IF;
END $$;
SQL
printf 'Comment history upgrade, rollback, bounded expiry and restore passed\n'
