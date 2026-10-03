#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster as root.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
port=${BOARD_TEST_PORT:-55432}
[[ $port =~ ^[0-9]{1,5}$ && $port -gt 0 && $port -lt 65536 ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p "$port" -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'The selected port belongs to another cluster.' >&2; exit 1; }
upgrade_db="imageboard_public_names_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_public_names_[0-9]+_[0-9]+$ ]] || exit 1
created=0
cleanup() {
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
    created=0
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public;
SQL
created=1
export OWNED_NAME_UPGRADE_DATABASE="$upgrade_db"
upgrade_url=$(python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ['MIGRATION_DATABASE_URL'])
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_NAME_UPGRADE_DATABASE'])))
PY
)
public_url=$(python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ['TEST_PUBLIC_DATABASE_URL'])
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_NAME_UPGRADE_DATABASE'])))
PY
)
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0067_public_display_names.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('names','Owned name upgrade','Owned synthetic fixture',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7971,'names','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7971,'names',7971,'Historical#literal','Historical subject','Owned historical text','2026-01-01Z');
CREATE TABLE public.owned_name_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_name_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_name_acl_before AS
  SELECT relacl::text AS acl FROM pg_catalog.pg_class WHERE oid='content.posts'::regclass;
CREATE TABLE public.owned_name_columns_before AS
  SELECT attname,attacl::text AS acl FROM pg_catalog.pg_attribute
  WHERE attrelid='content.posts'::regclass AND attnum>0 AND NOT attisdropped;
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7972,'names',7971,'','','Owned empty-name control');
    RAISE EXCEPTION 'Old constraint accepted an empty name' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7973,'names',7971,'A'||repeat(' ',252)||'B','','Owned expanded-name control');
    RAISE EXCEPTION 'Old constraint accepted an expanded name' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0067_public_display_names.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_name_posts_before)
    OR EXISTS(SELECT * FROM public.owned_name_posts_before EXCEPT SELECT * FROM content.posts)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_name_threads_before)
    OR EXISTS(SELECT * FROM public.owned_name_threads_before EXCEPT SELECT * FROM content.threads)
  THEN RAISE EXCEPTION 'Name migration changed historical content, identities or clocks'; END IF;
  IF EXISTS(SELECT relacl::text FROM pg_catalog.pg_class WHERE oid='content.posts'::regclass
      EXCEPT SELECT * FROM public.owned_name_acl_before)
    OR EXISTS(SELECT attname,attacl::text FROM pg_catalog.pg_attribute
      WHERE attrelid='content.posts'::regclass AND attnum>0 AND NOT attisdropped
      EXCEPT SELECT * FROM public.owned_name_columns_before)
  THEN RAISE EXCEPTION 'Name migration changed runtime ACLs'; END IF;
  IF has_column_privilege('board_public','content.posts','trip','INSERT,UPDATE')
    OR has_column_privilege('board_public','content.posts','name','UPDATE')
    OR has_table_privilege('board_public','content.boards','UPDATE')
    OR has_function_privilege('board_public','content.apply_post_trip()','EXECUTE')
  THEN RAISE EXCEPTION 'Name migration granted identity or policy mutation'; END IF;
END $$;
SQL
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7972,'names',7971,'','','Owned trip-only name');
COMMIT;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7973,'names',7971,'A'||repeat(' ',252)||'B','','Owned expanded name');
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7972 AND name='' AND trip='!ozOtJW9BFA')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7973 AND octet_length(name)=254 AND trip IS NULL)
  THEN RAISE EXCEPTION 'Name migration lost trip-only or expanded display names'; END IF;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7974,'names',7971,repeat('n',256),'','Owned excessive name');
    RAISE EXCEPTION 'Name storage bound was bypassed' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.posts SET name='Changed historical name' WHERE id=7971;
    RAISE EXCEPTION 'Public changed a saved name' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.posts SET trip='!ozOtJW9BFA' WHERE id=7971;
    RAISE EXCEPTION 'Public changed a saved trip' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Public display-name upgrade passed: retained history/clocks/ACLs, trip-only names, source spacing, storage limits and denied identity changes.'
