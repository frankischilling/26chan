#!/usr/bin/env bash
# Owned disposable cluster: populated 0154 upgrade and administrator restore.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
cluster=$(mktemp -d /tmp/board-homepage-upgrade.XXXXXXXX)
runner=()
if [[ $(id -u) = 0 ]]; then
    chown postgres:postgres "$cluster"
    runner=(runuser -u postgres --)
fi
started=0
cleanup() {
    status=$?
    trap - EXIT
    [[ $cluster =~ ^/tmp/board-homepage-upgrade\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
"${runner[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-c listen_addresses='127.0.0.1' -c port=55446 -c unix_socket_directories='' -c statement_timeout=30000 -c lock_timeout=5000" -w start >/dev/null
psql=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 55446 -U postgres)
"${psql[@]}" -d postgres -f - < deploy/roles.sql >/dev/null
"${psql[@]}" -d postgres <<'SQL'
CREATE DATABASE homepage_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE homepage_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE homepage_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d homepage_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0155_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('homeup','Owned homepage upgrade','Retained description',1000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(15500001,'homeup');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(15500001,'homeup',15500001,'Anonymous','Retained subject','Retained body');
CREATE TABLE public.homepage_saved_boards AS SELECT slug,to_jsonb(b) saved FROM content.boards b;
CREATE TABLE public.homepage_saved_threads AS SELECT id,to_jsonb(t) saved FROM content.threads t;
CREATE TABLE public.homepage_saved_posts AS SELECT id,to_jsonb(p) saved FROM content.posts p;
CREATE SCHEMA site_private;
SQL
if "${migrator[@]}" --single-transaction -f - < migrations/0155_local_site_content.sql >"$cluster/expected-conflict.log" 2>&1; then
    echo 'Existing schema conflict unexpectedly succeeded.' >&2; exit 1
fi
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF to_regclass('content.published_news') IS NOT NULL OR to_regclass('site_private.pages') IS NOT NULL THEN
 RAISE EXCEPTION 'Schema conflict did not roll back'; END IF;
END $$;
DROP SCHEMA site_private;
SQL
for migration in migrations/0155_*.sql migrations/0156_*.sql; do
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
cat > "$cluster/assertions.sql" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.homepage_saved_boards old FULL JOIN content.boards current USING(slug) WHERE old.saved IS DISTINCT FROM to_jsonb(current))
 OR EXISTS(SELECT 1 FROM public.homepage_saved_threads old FULL JOIN content.threads current USING(id) WHERE old.saved IS DISTINCT FROM to_jsonb(current))
 OR EXISTS(SELECT 1 FROM public.homepage_saved_posts old FULL JOIN content.posts current USING(id) WHERE old.saved IS DISTINCT FROM to_jsonb(current)) THEN RAISE EXCEPTION 'Existing content changed'; END IF;
 IF EXISTS(SELECT 1 FROM site_private.pages) OR EXISTS(SELECT 1 FROM site_private.news)
 OR EXISTS(SELECT 1 FROM site_private.frontpage_state) OR EXISTS(SELECT 1 FROM site_private.frontpage_cards)
 OR EXISTS(SELECT 1 FROM site_private.board_statistics) THEN RAISE EXCEPTION 'Upgrade invented site content'; END IF;
END $$;
SQL
"${migrator[@]}" -f - < "$cluster/assertions.sql"
"$pg_bin/pg_dump" -h 127.0.0.1 -p 55446 -U postgres -d homepage_upgrade --format=custom -f "$cluster/owned.dump"
"${psql[@]}" -d postgres -c 'CREATE DATABASE homepage_restored OWNER board_migrator'
"$pg_bin/pg_restore" -h 127.0.0.1 -p 55446 -U postgres -d homepage_restored --exit-on-error "$cluster/owned.dump"
"${psql[@]}" -d homepage_restored -f - < "$cluster/assertions.sql"
for database in homepage_upgrade homepage_restored; do
    "${psql[@]}" -d "$database" -At -f crates/store/src/homepage_readiness.sql | grep -qx t
    "${psql[@]}" -U board_public -d "$database" -c 'SELECT * FROM content.published_news; SELECT * FROM content.published_site_pages; SELECT * FROM content.homepage_cards; SELECT * FROM content.homepage_statistics; SELECT * FROM content.homepage_board_statistics' >/dev/null
 done
echo 'Populated homepage upgrade, conflict rollback, public projections and administrator restore passed.'
