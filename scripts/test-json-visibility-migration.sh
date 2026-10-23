#!/usr/bin/env bash
# Upgrade an owned 0247 database, reject drift, and preserve prior data/authority.
set -euo pipefail
cd "$(dirname "$0")/.."
pg_bin=${PG_BIN:-/usr/lib/postgresql/16/bin}
port=${JSON_VISIBILITY_TEST_PORT:-15486}
[[ $port =~ ^[0-9]+$ && $port -ge 1024 && $port -le 65535 ]] || exit 2
cluster=$(mktemp -d /tmp/board-json-visibility.XXXXXXXX)
as_pg=()
pg_user=$(id -un)
if [[ $(id -u) = 0 ]]; then
  chown postgres:postgres "$cluster"
  as_pg=(runuser -u postgres --)
  pg_user=postgres
fi
started=0
cleanup() {
  if [[ $started = 1 ]]; then
    "${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -m fast -w stop >/dev/null
  fi
  [[ $cluster =~ ^/tmp/board-json-visibility\.[[:alnum:]]{8}$ && -d $cluster && ! -L $cluster ]] || exit 1
  [[ $(readlink -f "$cluster") = "$cluster" ]] || exit 1
  rm -rf -- "$cluster"
}
trap cleanup EXIT
"${as_pg[@]}" "$pg_bin/initdb" -D "$cluster/data" --auth=trust --encoding=UTF8 --no-locale >"$cluster/init.log"
"${as_pg[@]}" "$pg_bin/pg_ctl" -D "$cluster/data" -l "$cluster/server.log" -o "-h 127.0.0.1 -p $port -k ''" -w start >/dev/null
started=1
admin=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U "$pg_user" -d postgres)
"${admin[@]}" -f deploy/roles.sql >"$cluster/roles.log"
"${admin[@]}" -c 'CREATE DATABASE owned_json_visibility OWNER board_migrator;' \
  -c 'REVOKE ALL ON DATABASE owned_json_visibility FROM PUBLIC;' \
  -c 'GRANT CONNECT ON DATABASE owned_json_visibility TO board_migrator,board_public;' >/dev/null
migrator=("$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_migrator -d owned_json_visibility)
for file in migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  [[ $file < migrations/0248_ ]] || break
  "${migrator[@]}" --single-transaction -f "$file" >>"$cluster/upgrade.log" 2>&1
done
"${migrator[@]}" >"$cluster/seed.log" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('ownjson','Owned JSON upgrade','Historical policy',1000,100,50,10,10);
INSERT INTO content.threads(id,board) VALUES(8891001,'ownjson');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 VALUES(8891001,'ownjson',8891001,'Owned','Historical subject','Historical comment');
INSERT INTO post_secrets.poster_contexts(post_id,thread_id,fingerprint,epoch)
 VALUES(8891001,8891001,decode(repeat('11',32),'hex'),decode(repeat('22',32),'hex'));
CREATE TABLE public.owned_json_before AS SELECT
 (SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b) AS boards,
 (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='ownjson') AS posts,
 (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board='ownjson') AS threads,
 (SELECT jsonb_agg(to_jsonb(c) ORDER BY post_id) FROM post_secrets.poster_contexts c WHERE thread_id=8891001) AS contexts;
CREATE TABLE public.owned_json_acl_before AS SELECT oid,relowner,relacl::text FROM pg_class WHERE relnamespace='content'::regnamespace;
SQL
for drift in "ALTER TABLE content.boards ALTER COLUMN title SET DEFAULT 'owned drift';" \
             "REVOKE SELECT ON content.posts FROM board_public;"; do
  printf '%s\n' "$drift" >"$cluster/drift.sql"
  if "${migrator[@]}" --single-transaction -f "$cluster/drift.sql" -f migrations/0248_source_json_visibility.sql >"$cluster/rejected.log" 2>&1; then
    echo 'JSON visibility migration accepted pre-existing authority drift' >&2; exit 1
  fi
  grep -q 'Transfer authority drifted before JSON visibility policy' "$cluster/rejected.log"
done
"${migrator[@]}" -c "DO \$\$ BEGIN IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attname='show_thread_uniques' AND NOT attisdropped) THEN RAISE EXCEPTION 'Rejected upgrade committed columns'; END IF; END \$\$;" >/dev/null
"${migrator[@]}" --single-transaction -f migrations/0248_source_json_visibility.sql >"$cluster/accepted.log"
"${migrator[@]}" <<'SQL'
DO $$
DECLARE before_row record;
BEGIN
 SELECT * INTO before_row FROM public.owned_json_before;
 IF before_row.boards IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(b)-ARRAY['show_thread_uniques','json_indexes_enabled','json_catalog_enabled','json_threads_enabled'] ORDER BY slug) FROM content.boards b)
 OR before_row.posts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board='ownjson')
 OR before_row.threads IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board='ownjson')
 OR before_row.contexts IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(c) ORDER BY post_id) FROM post_secrets.poster_contexts c WHERE thread_id=8891001) THEN
  RAISE EXCEPTION 'JSON visibility upgrade changed historical data'; END IF;
 IF EXISTS(SELECT 1 FROM public.owned_json_acl_before b JOIN pg_class c ON c.oid=b.oid
  WHERE b.relowner<>c.relowner OR b.relacl IS DISTINCT FROM c.relacl::text) THEN
  RAISE EXCEPTION 'JSON visibility upgrade changed content ownership or table grants'; END IF;
 IF EXISTS(SELECT 1 FROM content.boards WHERE show_thread_uniques
   OR json_indexes_enabled<>(slug NOT IN('j','test'))
   OR json_catalog_enabled<>(slug NOT IN('j','test'))
   OR json_threads_enabled<>(slug NOT IN('j','test'))) THEN
  RAISE EXCEPTION 'Imported JSON visibility differs from source'; END IF;
END $$;
SQL
if "$pg_bin/psql" -X -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$port" -U board_public -d owned_json_visibility \
  -c "UPDATE content.boards SET show_thread_uniques=true,json_catalog_enabled=true WHERE slug='ownjson'" >"$cluster/public-write.log" 2>&1; then
  echo 'Public runtime changed operator JSON visibility' >&2; exit 1
fi
grep -q 'permission denied' "$cluster/public-write.log"
echo 'JSON visibility upgrade, drift rejection, history preservation and restricted policy writes passed.'
