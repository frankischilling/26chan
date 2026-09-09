#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || { echo 'Expected a disposable cluster path.' >&2; exit 1; }
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_comment_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_comment_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
latin_db="${upgrade_db}_latin1"
created=0
latin_created=0
cleanup() {
  if [[ $created = 1 ]]; then
    "${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
DROP DATABASE :"upgrade_db";
SQL
    created=0
  fi
  if [[ $latin_created = 1 ]]; then
    "${admin[@]}" -v latin_db="$latin_db" <<'SQL'
DROP DATABASE :"latin_db";
SQL
    latin_created=0
  fi
}
trap cleanup EXIT
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
CREATE DATABASE :"upgrade_db" OWNER board_migrator TEMPLATE template0 ENCODING 'UTF8';
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator, board_public;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/000{1,2,3,4,5,6}_*.sql; do
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_bytes,reply_limit,bump_limit,thread_limit,threads_per_page)
  VALUES ('upgrade','Migration exercise','Synthetic fixture',16000,100,100,100,10),
         ('small','Small board','Synthetic fixture',4,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES (7001,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES (7001,'upgrade',7001,'Anonymous','',repeat('é',8000)),
         (7002,'upgrade',7001,'Anonymous','',E' before\r\ne'||chr(769)||' after ');
CREATE TABLE public.comment_upgrade_baseline AS SELECT id,comment FROM content.posts;
SQL
"${db[@]}" --single-transaction -f migrations/0007_comment_characters.sql
"${db[@]}" <<'SQL'
DO $$
BEGIN
  IF (SELECT count(*) FROM content.posts) <> 2 OR EXISTS (
    SELECT 1 FROM content.posts p FULL JOIN public.comment_upgrade_baseline b USING(id)
    WHERE p.comment IS DISTINCT FROM b.comment
  ) THEN RAISE EXCEPTION 'Migration changed historical text'; END IF;
  IF (SELECT max_comment_chars FROM content.boards WHERE slug='upgrade') <> 16000
    OR (SELECT max_comment_chars FROM content.boards WHERE slug='small') <> 4
  THEN RAISE EXCEPTION 'Migration changed numeric board limits'; END IF;
  IF NOT has_column_privilege('board_public','content.boards','max_comment_chars','SELECT')
    OR has_column_privilege('board_public','content.boards','max_comment_chars','UPDATE')
    OR NOT has_column_privilege('board_public','content.boards','slug','UPDATE')
  THEN RAISE EXCEPTION 'Migration changed runtime setting permissions'; END IF;
END $$;
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES (7003,'upgrade',7001,'Anonymous','',repeat(chr(128512),16000));
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM content.posts WHERE id=7003 AND char_length(comment)=16000 AND octet_length(comment)=64000)
  THEN RAISE EXCEPTION 'Upgraded runtime role cannot store the full character limit'; END IF;
END $$;
SQL
"${admin[@]}" -v latin_db="$latin_db" <<'SQL'
CREATE DATABASE :"latin_db" OWNER board_migrator TEMPLATE template0 ENCODING 'LATIN1' LC_COLLATE 'C' LC_CTYPE 'C';
REVOKE ALL ON DATABASE :"latin_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"latin_db" TO board_migrator;
SQL
latin_created=1
latin_url="${MIGRATION_DATABASE_URL%/imageboard}/$latin_db"
if "$pg_bin/psql" "$latin_url" -Xq -v ON_ERROR_STOP=1 --single-transaction -f migrations/0007_comment_characters.sql >.local/comment-migration-encoding.log 2>&1; then
  echo 'Migration unexpectedly accepted LATIN1.' >&2
  exit 1
fi
grep -Fq 'Migration 0007 requires UTF8 database encoding' .local/comment-migration-encoding.log
cleanup
printf 'Comment migration exercise passed: 0006 text and numeric settings preserved; runtime grants retained; 64,000-byte Unicode comment persisted; LATIN1 rejected. Disposable databases removed.\n'
