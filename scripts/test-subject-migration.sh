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
upgrade_db="imageboard_subject_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_subject_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0029_subject_spacing.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Subject upgrade','Owned synthetic fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(7701,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7701,'upgrade',7701,'Anonymous',repeat('s',120),'Historical subject');
CREATE TABLE public.owned_subject_before AS SELECT subject,created_at FROM content.posts WHERE id=7701;
CREATE TABLE public.owned_thread_before AS SELECT * FROM content.threads WHERE id=7701;
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7702,'upgrade',7701,'Anonymous','A'||repeat(' ',392)||'B','Before upgrade');
    RAISE EXCEPTION 'Old schema accepted source tab expansion' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0029_subject_spacing.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT subject,created_at FROM content.posts WHERE id=7701 EXCEPT SELECT * FROM public.owned_subject_before)
    OR EXISTS(SELECT * FROM content.threads WHERE id=7701 EXCEPT SELECT * FROM public.owned_thread_before)
  THEN RAISE EXCEPTION 'Subject migration rewrote historical values or clocks'; END IF;
END $$;
SQL
"${runtime[@]}" <<'SQL'
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7702,'upgrade',7701,'Anonymous','A'||repeat(' ',392)||'B','Expanded subject'),
      (7703,'upgrade',7701,'Anonymous',repeat('x',400),'Storage boundary');
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7702 AND subject='A'||repeat(' ',392)||'B')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7701 AND subject=repeat('s',120))
  THEN RAISE EXCEPTION 'Expanded or historical subject changed'; END IF;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7704,'upgrade',7701,'Anonymous',repeat('x',401),'Beyond storage boundary');
    RAISE EXCEPTION 'Subject storage is unbounded' USING ERRCODE='ZX001';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.posts SET subject='rewritten' WHERE id=7701;
    RAISE EXCEPTION 'Public rewrote historical subject' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.posts DROP CONSTRAINT posts_subject_check;
    RAISE EXCEPTION 'Public changed subject storage bound' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Subject upgrade passed: historical text/clocks retained, expanded and bounded subjects admitted, oversized storage and public mutation denied. Disposable database removed.'
