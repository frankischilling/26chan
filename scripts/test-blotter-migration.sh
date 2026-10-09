#!/usr/bin/env bash
# Owned disposable PostgreSQL only: populated 0117 -> 0118 and route-conflict rollback.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root on an owned disposable host.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
cluster=$(mktemp -d /tmp/board-blotter.XXXXXXXX)
started=0
cleanup() {
    status=$?
    trap - EXIT
    if [[ $started = 1 ]]; then
        runuser -u postgres -- "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null || status=1
    fi
    [[ $cluster =~ ^/tmp/board-blotter\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
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
CREATE DATABASE blotter_upgrade OWNER board_migrator;
REVOKE ALL ON DATABASE blotter_upgrade FROM PUBLIC;
GRANT CONNECT ON DATABASE blotter_upgrade TO board_migrator,board_public;
SQL
migrator=("${psql[@]}" -U board_migrator -d blotter_upgrade)
for migration in migrations/*.sql; do
    [[ $(basename "$migration") < 0118_ ]] || break
    "${migrator[@]}" --single-transaction -f - < "$migration" >/dev/null
done
"${migrator[@]}" -f - < fixtures/demo.sql >/dev/null
"${migrator[@]}" <<'SQL'
CREATE TABLE public.blotter_upgrade_boards AS SELECT slug,to_jsonb(b) saved FROM content.boards b;
CREATE TABLE public.blotter_upgrade_threads AS SELECT id,to_jsonb(t) saved FROM content.threads t;
CREATE TABLE public.blotter_upgrade_posts AS SELECT id,to_jsonb(p) saved FROM content.posts p;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('blotter','Owned conflict','Owned route-conflict fixture',1000,100,100,100,10);
SQL
if "${migrator[@]}" --single-transaction -f - < migrations/0118_local_blotter.sql >"$cluster/expected-conflict.log" 2>&1; then
    echo 'Reserved route conflict unexpectedly succeeded.' >&2; exit 1
fi
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname='show_blotter' AND NOT attisdropped)
 OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='blotter' AND title='Owned conflict') THEN
 RAISE EXCEPTION 'Conflict did not roll back atomically'; END IF;
END $$;
DELETE FROM content.boards WHERE slug='blotter' AND title='Owned conflict';
SQL
"${migrator[@]}" --single-transaction -f - < migrations/0118_local_blotter.sql
"${migrator[@]}" <<'SQL'
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.blotter_upgrade_boards before FULL JOIN content.boards after USING(slug)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)-'show_blotter') THEN RAISE EXCEPTION 'Board data changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.blotter_upgrade_threads before FULL JOIN content.threads after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Thread data changed'; END IF;
 IF EXISTS(SELECT 1 FROM public.blotter_upgrade_posts before FULL JOIN content.posts after USING(id)
 WHERE before.saved IS DISTINCT FROM to_jsonb(after)) THEN RAISE EXCEPTION 'Post data changed'; END IF;
 IF EXISTS(SELECT 1 FROM content.boards WHERE show_blotter IS DISTINCT FROM (slug<>'j')) THEN RAISE EXCEPTION 'Policy differs'; END IF;
 IF EXISTS(SELECT 1 FROM blotter_private.messages) THEN RAISE EXCEPTION 'Migration seeded messages'; END IF;
END $$;
SQL
# The current exact projection/grant probe is checked using the public role.
python3 - <<'PY' | "${psql[@]}" -U board_public -d blotter_upgrade
import pathlib,re
source=pathlib.Path('crates/store/src/blotter.rs').read_text()
sql=re.search(r'pub const BLOTTER_READINESS_SQL: &str = r#"(.*?)"#;',source,re.S).group(1)
print("DO $$ BEGIN IF ("+sql+") IS DISTINCT FROM true THEN RAISE EXCEPTION 'Blotter readiness failed'; END IF; END $$;")
PY
printf 'Blotter upgrade, reserved-route rollback, source policy and public projection passed.\n'
