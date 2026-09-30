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
upgrade_db="imageboard_tripcodes_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_tripcodes_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0038_tripcodes.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,forced_anon)
VALUES('trip','Owned identity upgrade','Owned fixture',1000,100,100,100,10,false),
      ('anon','Owned anonymous upgrade','Owned fixture',1000,100,100,100,10,true);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7901,'trip','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7901,'trip',7901,'Historical name','Historical subject','Owned historical comment','2026-01-01Z');
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0038_tripcodes.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT to_jsonb(p)-'trip' FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_posts_before p)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
    OR EXISTS(SELECT 1 FROM content.posts WHERE trip IS NOT NULL)
  THEN RAISE EXCEPTION 'Tripcode migration changed historical fields or clocks'; END IF;
  BEGIN
    UPDATE content.posts SET trip='untrusted-trip' WHERE id=7901;
    RAISE EXCEPTION 'Invalid trip admitted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  IF has_column_privilege('board_public','content.posts','trip','INSERT,UPDATE')
    OR has_function_privilege('board_public','content.apply_post_trip()','EXECUTE')
    OR has_function_privilege('board_attachment_owner','content.apply_post_trip()','EXECUTE')
  THEN RAISE EXCEPTION 'Tripcode migration expanded unrelated runtime authority'; END IF;
END $$;
-- Healthy controls for the same mutations subsequently denied to public SQL.
BEGIN;
UPDATE content.posts SET trip='!ozOtJW9BFA' WHERE id=7901;
ROLLBACK;
SQL
runtime=("$pg_bin/psql" "${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7902,'trip',7901,'Owned named reply','','Owned reply');
INSERT INTO content.threads(id,board) VALUES(7903,'anon');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7903,'anon',7903,'Discarded name','','Owned forced-anonymous OP');
COMMIT;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7904,'trip',7901,'Owned plain reply','','Owned plain reply');
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7902 AND trip='!ozOtJW9BFA')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7903 AND trip IS NULL AND name='Anonymous')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7904 AND trip IS NULL)
  THEN RAISE EXCEPTION 'Tripcode trigger lost identity, forced anonymity or transaction isolation'; END IF;
  BEGIN
    UPDATE content.posts SET trip='!!XYWOFgjf7hP' WHERE id=7901;
    RAISE EXCEPTION 'Public changed a saved identity' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment,trip)
    VALUES(7905,'trip',7901,'Injected','','Owned text','!!XYWOFgjf7hP');
    RAISE EXCEPTION 'Public inserted a trip field directly' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Tripcode upgrade passed: retained history/clocks, bounded identities, forced anonymity, transaction reuse and denied identity mutation. Disposable database removed.'
