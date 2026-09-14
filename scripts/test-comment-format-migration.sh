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
upgrade_db="imageboard_comment_format_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_comment_format_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0031_post_comment_format.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,
    comment_spoiler_cleanup,comment_code_spacing,comment_sjis_spacing)
SELECT 'fmt'||mask,'Format upgrade','Owned fixture',1000,100,100,100,10,
    (mask & 1)<>0,(mask & 2)<>0,(mask & 4)<>0 FROM generate_series(0,7) mask;
INSERT INTO content.threads(id,board,created_at,modified_at)
SELECT 8100+mask,'fmt'||mask,'2026-01-01Z','2026-01-02Z' FROM generate_series(0,7) mask;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
SELECT 8100+mask,'fmt'||mask,8100+mask,'Anonymous','','[spoiler]Historical <b>text</b>[/spoiler]',
    '2026-01-01Z' FROM generate_series(0,7) mask;
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0031_post_comment_format.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT to_jsonb(p)-'comment_format' FROM content.posts p EXCEPT SELECT to_jsonb(p) FROM public.owned_posts_before p)
    OR EXISTS(SELECT * FROM content.boards EXCEPT SELECT * FROM public.owned_boards_before)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
    OR EXISTS(SELECT 1 FROM content.posts WHERE comment_format<>0)
  THEN RAISE EXCEPTION 'Format upgrade changed history or existing policy'; END IF;
  BEGIN
    UPDATE content.posts SET comment_format=7 WHERE id=8100;
    RAISE EXCEPTION 'Invalid format version accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.posts SET comment_format=NULL WHERE id=8100;
    RAISE EXCEPTION 'Null format version accepted' USING ERRCODE='ZX001';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
END $$;
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
SELECT 8200+mask,'fmt'||mask,8100+mask,'Anonymous','','Owned new markup post' FROM generate_series(0,7) mask;
DO $$ BEGIN
  IF (SELECT count(*) FROM content.posts WHERE id BETWEEN 8200 AND 8207)<>8
    OR EXISTS(SELECT 1 FROM content.posts WHERE id BETWEEN 8200 AND 8207 AND comment_format<>8+id-8200)
    OR EXISTS(SELECT 1 FROM content.posts WHERE id BETWEEN 8100 AND 8107 AND comment_format<>0)
  THEN RAISE EXCEPTION 'Posting-time stamps or historical format differ'; END IF;
  BEGIN
    UPDATE content.posts SET comment_format=0 WHERE id=8200;
    RAISE EXCEPTION 'Public rewrote format history' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment,comment_format)
      VALUES(8300,'fmt0',8100,'Anonymous','','Forged format',15);
    RAISE EXCEPTION 'Public selected format authority' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.posts DISABLE TRIGGER stamp_comment_format;
    RAISE EXCEPTION 'Public disabled format trigger' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${db[@]}" <<'SQL'
UPDATE content.boards SET comment_spoiler_cleanup=false,comment_code_spacing=false,comment_sjis_spacing=false;
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.posts WHERE id BETWEEN 8200 AND 8207 AND comment_format<>8+id-8200)
    OR EXISTS(SELECT 1 FROM content.posts WHERE id BETWEEN 8100 AND 8107 AND comment_format<>0)
  THEN RAISE EXCEPTION 'Board change reinterpreted format history'; END IF;
END $$;
SQL
cleanup
echo 'Comment format upgrade passed: retained history/clocks, eight posting-time policies, invalid-version checks and denied public format/schema writes. Disposable database removed.'
