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
upgrade_db="imageboard_required_subject_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_required_subject_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0030_required_subject.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
-- All 82 active source board filenames. Only qst/vg enable REQUIRE_SUBJECT.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT slug,'Subject policy upgrade','Owned fixture',1000,100,100,100,10 FROM unnest(ARRAY[
'3','a','aco','adv','an','asp','b','bant','biz','c','cgl','ck','cm','co','d','diy','e','f','fa','fit','g','gd','gif','h','hc','his','hm','hr','i','ic','int','j','jp','k','lgbt','lit','m','mlp','mu','n','news','o','out','p','po','pol','pw','qa','qb','qst','r','r9k','s','s4s','sci','soc','sp','t','test','tg','toy','trash','trv','tv','u','v','vg','vip','vm','vmg','vp','vr','vrpg','vst','vt','w','wg','wsg','wsr','x','xs','y'
]) AS source(slug);
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7801,'qst','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7801,'qst',7801,'Anonymous','','Historical subjectless OP','2026-01-01Z');
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0030_required_subject.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards) <> 82
    OR EXISTS(SELECT 1 FROM content.boards WHERE require_subject IS DISTINCT FROM (slug IN ('qst','vg')))
  THEN RAISE EXCEPTION 'Required-subject source defaults differ'; END IF;
  IF EXISTS(SELECT to_jsonb(b)-'require_subject' FROM content.boards b EXCEPT SELECT to_jsonb(b) FROM public.owned_boards_before b)
    OR EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
  THEN RAISE EXCEPTION 'Policy upgrade rewrote existing board fields, posts or clocks'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('future','Future policy','Owned fixture',1000,100,100,100,10);
DO $$ BEGIN
  IF (SELECT require_subject FROM content.boards WHERE slug='future') IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'Future boards did not inherit the global default'; END IF;
  BEGIN
    UPDATE content.boards SET require_subject=NULL WHERE slug='future';
    RAISE EXCEPTION 'Required-subject policy became nullable' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
UPDATE content.boards SET require_subject=true WHERE slug='future';
DO $$ BEGIN
  IF NOT (SELECT require_subject FROM content.boards WHERE slug='future')
  THEN RAISE EXCEPTION 'Operator could not enable subject policy'; END IF;
END $$;
UPDATE content.boards SET require_subject=false WHERE slug='future';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT require_subject FROM content.boards WHERE slug='future') IS DISTINCT FROM false
    OR NOT (SELECT require_subject FROM content.boards WHERE slug='qst')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7801 AND subject='')
  THEN RAISE EXCEPTION 'Public reads lost policy or historical content'; END IF;
  BEGIN
    UPDATE content.boards SET require_subject=false WHERE slug='qst';
    RAISE EXCEPTION 'Public changed subject policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.boards DROP COLUMN require_subject;
    RAISE EXCEPTION 'Public changed subject schema' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Required-subject upgrade passed: 82 source defaults, retained historical content/clocks, operator changes and denied public policy/schema writes. Disposable database removed.'
