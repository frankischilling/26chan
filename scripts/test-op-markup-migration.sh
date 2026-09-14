#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_op_markup_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_op_markup_[0-9]+_[0-9]+$ ]] || exit 1
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
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0034_op_markup.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
-- All 82 active source board filenames. Only qst/test enable OP_MARKUP.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT slug,'OP markup upgrade','Owned fixture',1000,100,100,100,10 FROM unnest(ARRAY[
'3','a','aco','adv','an','asp','b','bant','biz','c','cgl','ck','cm','co','d','diy','e','f','fa','fit','g','gd','gif','h','hc','his','hm','hr','i','ic','int','j','jp','k','lgbt','lit','m','mlp','mu','n','news','o','out','p','po','pol','pw','qa','qb','qst','r','r9k','s','s4s','sci','soc','sp','t','test','tg','toy','trash','trv','tv','u','v','vg','vip','vm','vmg','vp','vr','vrpg','vst','vt','w','wg','wsg','wsr','x','xs','y'
]) AS source(slug);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7801,'qst','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7801,'qst',7801,'Anonymous','','Historical subjectless OP','2026-01-01Z');
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0034_op_markup.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards) <> 82
    OR EXISTS(SELECT 1 FROM content.boards WHERE op_markup IS DISTINCT FROM (slug IN ('qst','test')))
  THEN RAISE EXCEPTION 'OP-markup source defaults differ'; END IF;
  IF EXISTS(SELECT to_jsonb(b)-'op_markup' FROM content.boards b EXCEPT SELECT to_jsonb(b) FROM public.owned_boards_before b)
    OR EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
  THEN RAISE EXCEPTION 'Policy upgrade rewrote existing board fields, posts or clocks'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('future','Future policy','Owned fixture',1000,100,100,100,10);
DO $$ BEGIN
  IF (SELECT op_markup FROM content.boards WHERE slug='future') IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'Future boards did not inherit the global default'; END IF;
  BEGIN
    UPDATE content.boards SET op_markup=NULL WHERE slug='future';
    RAISE EXCEPTION 'OP-markup policy became nullable' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
UPDATE content.boards SET op_markup=true WHERE slug='future';
DO $$ BEGIN
  IF NOT (SELECT op_markup FROM content.boards WHERE slug='future')
  THEN RAISE EXCEPTION 'Operator could not enable OP markup policy'; END IF;
END $$;
UPDATE content.boards SET op_markup=false WHERE slug='future';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT op_markup FROM content.boards WHERE slug='future') IS DISTINCT FROM false
    OR NOT (SELECT op_markup FROM content.boards WHERE slug='qst')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7801 AND subject='')
  THEN RAISE EXCEPTION 'Public reads lost policy or historical content'; END IF;
  BEGIN
    UPDATE content.boards SET op_markup=false WHERE slug='qst';
    RAISE EXCEPTION 'Public changed OP markup policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.boards DROP COLUMN op_markup;
    RAISE EXCEPTION 'Public changed subject schema' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${runtime[@]}" <<'SQL'
BEGIN;
INSERT INTO content.threads(id,board) VALUES(7802,'qst'),(7805,'future');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(7802,'qst',7802,'Anonymous','','[b]owned OP[/b]');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(7803,'qst',7802,'Anonymous','','[b]ordinary reply[/b]');
SET LOCAL board.source_op_reply='true';
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(7804,'qst',7802,'Anonymous','','[b]server-selected reply[/b]');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(7805,'future',7805,'Anonymous','','[b]disabled board[/b]');
DO $$ BEGIN
  IF (SELECT comment_format FROM content.posts WHERE id=7802) <> 24
    OR (SELECT comment_format FROM content.posts WHERE id=7803) <> 8
    OR (SELECT comment_format FROM content.posts WHERE id=7804) <> 24
    OR (SELECT comment_format FROM content.posts WHERE id=7805) <> 8
  THEN RAISE EXCEPTION 'OP/reply/disabled-board stamps differ'; END IF;
  BEGIN
    UPDATE content.posts SET comment_format=24 WHERE id=7803;
    RAISE EXCEPTION 'Public rewrote a saved format' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
COMMIT;
DO $$ BEGIN
  IF current_setting('board.source_op_reply', true)='true'
  THEN RAISE EXCEPTION 'Transaction context leaked'; END IF;
END $$;
SQL
cleanup
echo 'OP-markup upgrade passed: 82 source defaults, retained historical content/clocks, operator changes and denied public policy/schema writes. Disposable database removed.'
