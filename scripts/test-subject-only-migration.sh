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
upgrade_db="imageboard_subject_only_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_subject_only_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0032_subject_only_posts.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Subject-only upgrade','Owned fixture',1000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(7901,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7901,'upgrade',7901,'Anonymous','','Historical comment'),
      (7902,'upgrade',7901,'Anonymous','','Historical reply');
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
SQL
"${db[@]}" --single-transaction -f migrations/0032_subject_only_posts.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
    OR EXISTS(SELECT * FROM public.owned_posts_before EXCEPT SELECT * FROM content.posts)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
  THEN RAISE EXCEPTION 'Admission upgrade changed historical content or clocks'; END IF;
  IF has_function_privilege('board_public','content.require_attachment_for_empty_post()','EXECUTE')
    OR has_function_privilege('board_migrator','content.require_attachment_for_empty_post()','EXECUTE')
    OR has_schema_privilege('board_attachment_owner','content','CREATE')
    OR has_table_privilege('board_public','content.posts','TRIGGER')
    OR has_column_privilege('board_public','content.posts','subject','UPDATE')
    OR has_column_privilege('board_public','content.posts','comment','UPDATE')
    OR has_table_privilege('board_public','content.post_media','INSERT,DELETE')
  THEN RAISE EXCEPTION 'Admission upgrade exposed authority'; END IF;
END $$;
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
BEGIN;
INSERT INTO content.threads(id,board) VALUES(7903,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7903,'upgrade',7903,'Anonymous','Subject without comment','');
COMMIT;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7904,'upgrade',7903,'Anonymous','','Healthy reply');
SQL
# Real commits as the runtime role must reject subject-only replies and wholly
# empty OPs. A healthy OP and reply above prove the grants and trigger execute.
for parent in 7903 7905; do
  if "${runtime[@]}" -v parent="$parent" >.local/subject-only-migration-denial.log 2>&1 <<'SQL'
BEGIN;
INSERT INTO content.threads(id,board) VALUES(7905,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7905,'upgrade',:parent,'Anonymous',CASE WHEN :parent=7903 THEN 'Reply subject' ELSE '' END,'');
COMMIT;
SQL
  then
    echo 'Runtime committed an unattached empty OP or subject-only reply.' >&2; exit 1
  fi
  grep -Fq 'An empty comment requires an authorized attachment.' .local/subject-only-migration-denial.log
done
# Operator updates queue deferred events. Every event must inspect the final
# row, including a subject cleared after insertion or in a later transaction.
for mutation in existing queued; do
  if [[ $mutation = existing ]]; then
    statement="UPDATE content.posts SET subject='' WHERE id=7903;"
  else
    statement="INSERT INTO content.threads(id,board) VALUES(7905,'upgrade'); INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(7905,'upgrade',7905,'Anonymous','Temporary subject',''); UPDATE content.posts SET subject='' WHERE id=7905;"
  fi
  if "${db[@]}" --single-transaction -c "$statement" >.local/subject-only-migration-denial.log 2>&1; then
    echo 'Clearing a subject bypassed deferred final-row admission.' >&2; exit 1
  fi
  grep -Fq 'An empty comment requires an authorized attachment.' .local/subject-only-migration-denial.log
done
"${db[@]}" <<'SQL'
BEGIN;
UPDATE content.posts SET subject='' WHERE id=7903;
UPDATE content.posts SET comment='Replacement content' WHERE id=7903;
COMMIT;
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.posts WHERE id=7905)
    OR EXISTS(SELECT 1 FROM content.threads WHERE id=7905)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7903 AND subject='' AND comment='Replacement content')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7904 AND comment='Healthy reply')
  THEN RAISE EXCEPTION 'Admission commit/rollback result differs'; END IF;
END $$;
SQL
cleanup
echo 'Subject-only upgrade passed: historical rows retained, real runtime OP/reply commits, empty denial, final-row update checks and unchanged authority. Disposable database removed.'
