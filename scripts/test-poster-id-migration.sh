#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
port=${BOARD_TEST_PORT:-55432}
[[ $port =~ ^[0-9]{1,5}$ && $port -gt 0 && $port -lt 65536 ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p "$port" -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'The configured port belongs to another cluster.' >&2; exit 1; }
upgrade_db="imageboard_poster_ids_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_poster_ids_[0-9]+_[0-9]+$ ]] || exit 1
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
db=("$pg_bin/psql" "${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0039_poster_ids.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('ids','Owned ID upgrade','Owned fixture',1000,100,100,100,10),('plain','Owned plain upgrade','Owned fixture',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(8901,'ids','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(8901,'ids',8901,'Historical name','Historical subject','Owned historical comment','2026-01-01Z');
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0039_poster_ids.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT to_jsonb(p)-'poster_id' FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_posts_before p)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
    OR EXISTS(SELECT 1 FROM content.posts WHERE poster_id IS NOT NULL)
    OR EXISTS(SELECT 1 FROM content.boards WHERE user_ids)
  THEN RAISE EXCEPTION 'ID migration changed historical content, defaults or clocks'; END IF;
  IF has_column_privilege('board_public','content.posts','poster_id','INSERT,UPDATE')
    OR has_column_privilege('board_public','content.boards','user_ids','UPDATE')
    OR has_function_privilege('board_public','content.apply_poster_id()','EXECUTE')
    OR has_function_privilege('board_attachment_owner','content.apply_poster_id()','EXECUTE')
  THEN RAISE EXCEPTION 'ID migration expanded runtime authority'; END IF;
  BEGIN
    UPDATE content.posts SET poster_id='invalid' WHERE id=8901;
    RAISE EXCEPTION 'Invalid ID admitted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
UPDATE content.boards SET user_ids=true WHERE slug='ids';
-- The identical owner operations succeed before runtime denial controls.
BEGIN;
UPDATE content.posts SET poster_id='AAAAAAAA' WHERE id=8901;
UPDATE content.boards SET user_ids=false WHERE slug='ids';
ROLLBACK;
SQL
runtime=("$pg_bin/psql" "${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.poster_id','AAAAAAAA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8902,'ids',8901,'Owned reply','','Owned reply');
INSERT INTO content.threads(id,board) VALUES(8903,'plain');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8903,'plain',8903,'Plain reply','','Owned text');
COMMIT;
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=8902 AND poster_id='AAAAAAAA')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=8903 AND poster_id IS NULL)
  THEN RAISE EXCEPTION 'ID trigger lost label or disabled-board policy'; END IF;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(8904,'ids',8901,'Missing identity','','Owned text');
    RAISE EXCEPTION 'ID escaped transaction or missing identity was admitted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.posts SET poster_id='BBBBBBBB' WHERE id=8901;
    RAISE EXCEPTION 'Runtime changed saved identity' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.boards SET user_ids=false WHERE slug='ids';
    RAISE EXCEPTION 'Runtime changed ID policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Poster ID upgrade passed: retained history/clocks, disabled defaults, label constraints, transaction isolation and runtime mutation denials. Disposable database removed.'
