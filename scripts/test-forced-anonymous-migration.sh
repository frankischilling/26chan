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
upgrade_db="imageboard_forced_anonymous_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_forced_anonymous_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0035_forced_anonymous.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT slug,'Forced anonymous upgrade','Owned fixture',1000,100,100,100,10 FROM unnest(ARRAY[
'3','a','aco','adv','an','asp','b','bant','biz','c','cgl','ck','cm','co','d','diy','e','f','fa','fit','g','gd','gif','h','hc','his','hm','hr','i','ic','int','j','jp','k','lgbt','lit','m','mlp','mu','n','news','o','out','p','po','pol','pw','qa','qb','qst','r','r9k','s','s4s','sci','soc','sp','t','test','tg','toy','trash','trv','tv','u','v','vg','vip','vm','vmg','vp','vr','vrpg','vst','vt','w','wg','wsg','wsr','x','xs','y'
]) AS source(slug);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7901,'test','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7901,'test',7901,'Historical name','Historical subject','Owned historical comment','2026-01-01Z');
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0035_forced_anonymous.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards) <> 82 OR EXISTS(SELECT 1 FROM content.boards WHERE forced_anon)
  THEN RAISE EXCEPTION 'Forced-anonymous source defaults differ'; END IF;
  IF EXISTS(SELECT to_jsonb(b)-'forced_anon' FROM content.boards b EXCEPT SELECT to_jsonb(b) FROM public.owned_boards_before b)
    OR EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
  THEN RAISE EXCEPTION 'Policy upgrade rewrote historical fields or clocks'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('future','Future policy','Owned fixture',1000,100,100,100,10);
DO $$ BEGIN
  IF (SELECT forced_anon FROM content.boards WHERE slug='future') IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'Future board default differs'; END IF;
  BEGIN
    UPDATE content.boards SET forced_anon=NULL WHERE slug='future';
    RAISE EXCEPTION 'Policy became nullable' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
UPDATE content.boards SET forced_anon=true WHERE slug='test';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT (SELECT forced_anon FROM content.boards WHERE slug='test')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7901 AND name='Historical name' AND subject='Historical subject')
  THEN RAISE EXCEPTION 'Public reads lost policy or history'; END IF;
  BEGIN
    UPDATE content.boards SET forced_anon=false WHERE slug='test';
    RAISE EXCEPTION 'Public changed policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.boards DROP COLUMN forced_anon;
    RAISE EXCEPTION 'Public changed schema' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
INSERT INTO content.threads(id,board) VALUES(7902,'test'),(7904,'future');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7902,'test',7902,'Discarded name','Discarded subject','Owned OP'),
      (7903,'test',7902,'Discarded reply name','Discarded reply subject','Owned reply'),
      (7904,'future',7904,'Retained name','Retained subject','Owned disabled-board OP');
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.posts WHERE id IN (7902,7903) AND (name<>'Anonymous' OR subject<>''))
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7904 AND name='Retained name' AND subject='Retained subject')
  THEN RAISE EXCEPTION 'Public direct insertion bypassed identity policy'; END IF;
END $$;
SQL
"${db[@]}" <<'SQL'
UPDATE content.boards SET forced_anon=false WHERE slug='test';
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.posts WHERE id IN (7902,7903) AND (name<>'Anonymous' OR subject<>''))
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7901 AND name='Historical name' AND subject='Historical subject')
  THEN RAISE EXCEPTION 'Policy toggle changed saved identities'; END IF;
END $$;
SQL
cleanup
echo 'Forced-anonymous upgrade passed: 82 defaults, retained history/clocks, runtime identity policy and denied writes. Disposable database removed.'
