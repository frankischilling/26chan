#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Unexpected database cluster.' >&2; exit 1; }
upgrade_db="imageboard_word_break_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_word_break_[0-9]+_[0-9]+$ ]] || exit 1
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
db=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="$upgrade_url")
for migration in migrations/*.sql; do
  [[ $migration != migrations/0036_comment_word_breaks.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('wordbreak','Word breaks','Owned upgrade fixture',1000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at)
SELECT 7900+n,'wordbreak','2026-01-01Z','2026-01-02Z' FROM generate_series(0,16) n;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT 7900+n,'wordbreak',7900+n,'Anonymous','Historical',repeat('x',70),'2026-01-01Z' FROM generate_series(0,16) n;
UPDATE content.posts SET comment_format=CASE WHEN id=7900 THEN 0 WHEN id<=7908 THEN id-7893 ELSE id-7885 END;
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0036_comment_word_breaks.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.posts)<>17
    OR EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
    OR EXISTS(SELECT * FROM public.owned_posts_before EXCEPT SELECT * FROM content.posts)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
    OR EXISTS(SELECT * FROM content.boards EXCEPT SELECT * FROM public.owned_boards_before)
  THEN RAISE EXCEPTION 'Historical profiles, content, policy or clocks changed'; END IF;
  IF NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='content' AND p.proname='stamp_comment_format' AND NOT p.prosecdef
      AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
      AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0))
  THEN RAISE EXCEPTION 'Stamp authority changed'; END IF;
END $$;
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 --dbname="$public_url")
"${runtime[@]}" <<'SQL'
BEGIN;
INSERT INTO content.threads(id,board) VALUES(8000,'wordbreak');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8000,'wordbreak',8000,'Anonymous','',repeat('x',70)),(8001,'wordbreak',8000,'Anonymous','','Reply');
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.posts WHERE id IN (8000,8001) AND comment_format<>40)
  THEN RAISE EXCEPTION 'Public OP/reply word-break profile missing'; END IF;
  BEGIN
    UPDATE content.posts SET comment_format=8 WHERE id=8001;
    RAISE EXCEPTION 'Public changed saved format' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format)
    VALUES(8099,'wordbreak',8000,'Anonymous','','Forged',0);
    RAISE EXCEPTION 'Public supplied a format override' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.posts DISABLE TRIGGER USER;
    RAISE EXCEPTION 'Public disabled stamping' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
COMMIT;
SQL
"${db[@]}" <<'SQL'
BEGIN;
SET LOCAL ROLE board_attachment_owner;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(8002,'wordbreak',8000,'Anonymous','','Scoped insert');
RESET ROLE;
DO $$ BEGIN
  IF (SELECT comment_format FROM content.posts WHERE id=8002)<>40
  THEN RAISE EXCEPTION 'Scoped insert lost the stamp'; END IF;
END $$;
UPDATE content.boards SET op_markup=true,comment_spoiler_cleanup=true,comment_code_spacing=true,comment_sjis_spacing=true WHERE slug='wordbreak';
INSERT INTO content.threads(id,board) VALUES(8003,'wordbreak');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format)
VALUES(8003,'wordbreak',8003,'Anonymous','','New OP',0),(8004,'wordbreak',8003,'Anonymous','','Other reply',0);
SET LOCAL board.source_op_reply='true';
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format)
VALUES(8005,'wordbreak',8003,'Anonymous','','Owned reply',0);
DO $$ BEGIN
  IF (SELECT comment_format FROM content.posts WHERE id=8003)<>63
    OR (SELECT comment_format FROM content.posts WHERE id=8004)<>47
    OR (SELECT comment_format FROM content.posts WHERE id=8005)<>63
  THEN RAISE EXCEPTION 'Existing markup bits or forced new profile differ'; END IF;
END $$;
UPDATE content.boards SET op_markup=false,comment_spoiler_cleanup=false,comment_code_spacing=false,comment_sjis_spacing=false WHERE slug='wordbreak';
DO $$ BEGIN
  IF EXISTS(SELECT * FROM content.posts WHERE id BETWEEN 7900 AND 7916 EXCEPT SELECT * FROM public.owned_posts_before)
    OR (SELECT comment_format FROM content.posts WHERE id=8003)<>63
  THEN RAISE EXCEPTION 'Later policy rewrote historical rendering'; END IF;
END $$;
COMMIT;
SQL
cleanup
echo 'Word-break upgrade passed: historical profiles retained, new posts stamped, public overrides denied, scoped insertion checked. Disposable database removed.'
