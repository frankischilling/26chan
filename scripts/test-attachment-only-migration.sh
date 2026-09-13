#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { printf 'Run as root against the disposable development cluster.\n' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { printf 'Disposable cluster identity mismatch.\n' >&2; exit 1; }
upgrade_db="imageboard_attachment_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_attachment_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator, board_public;
SQL
created=1
db=("$pg_bin/psql" "${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db" -Xq -v ON_ERROR_STOP=1)
applied=0
for migration in migrations/*.sql; do
  [[ ${migration##*/} == 0019_* ]] && break
  "${db[@]}" --single-transaction -f "$migration"
  applied=$((applied + 1))
done
[[ $applied = 18 ]] || { printf 'Expected the complete 0018 predecessor.\n' >&2; exit 1; }
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
  VALUES ('upgrade','Attachment upgrade','Synthetic',16000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES (7001,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES (7001,'upgrade',7001,'Anonymous','',repeat(chr(128512),16000)),
         (7002,'upgrade',7001,'Anonymous','',E' before\r\ne'||chr(769)||' after ');
CREATE TABLE public.attachment_comment_baseline AS SELECT id,comment FROM content.posts;
SQL
"${db[@]}" --single-transaction -f migrations/0019_attachment_only_posts.sql
"${db[@]}" <<'SQL'
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM content.posts p FULL JOIN public.attachment_comment_baseline b USING(id)
    WHERE p.comment IS DISTINCT FROM b.comment) THEN
    RAISE EXCEPTION 'Attachment migration changed historical text';
  END IF;
  IF has_function_privilege('board_public','content.require_attachment_for_empty_post()','EXECUTE')
    OR has_function_privilege('board_migrator','content.require_attachment_for_empty_post()','EXECUTE')
    OR has_table_privilege('board_public','content.posts','TRIGGER')
    OR has_column_privilege('board_public','content.posts','comment','UPDATE')
    OR has_table_privilege('board_public','content.post_media','INSERT,DELETE') THEN
    RAISE EXCEPTION 'Attachment migration exposed authority';
  END IF;
END $$;
SQL
public=("$pg_bin/psql" "${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db" -Xq -v ON_ERROR_STOP=1)
"${public[@]}" <<'SQL'
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES (7003,'upgrade',7001,'Anonymous','',repeat(chr(128512),16000));
SQL
if "${public[@]}" >.local/attachment-only-migration-denial.log 2>&1 <<'SQL'
BEGIN;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES (7004,'upgrade',7001,'Anonymous','','');
COMMIT;
SQL
then
  printf 'Public role committed an unattached empty post.\n' >&2
  exit 1
fi
grep -Fq 'An empty comment requires an authorized attachment.' .local/attachment-only-migration-denial.log
"${db[@]}" <<'SQL'
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM content.posts WHERE id=7004)
    OR NOT EXISTS (SELECT 1 FROM content.posts WHERE id=7003 AND char_length(comment)=16000 AND octet_length(comment)=64000)
  THEN RAISE EXCEPTION 'Post-upgrade commit or bounds regression'; END IF;
END $$;
SQL
cleanup
printf 'Attachment-only upgrade passed: 0018 text preserved, temporary authority revoked, full Unicode bounds retained, unattached empty commit rejected.\n'
