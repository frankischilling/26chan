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
upgrade_db="imageboard_field_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_field_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0021_public_name_limit.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Field upgrade','Owned synthetic upgrade fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(7001,'upgrade');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7001,'upgrade',7001,repeat('n',80),repeat('s',120),'Preserved old fields');
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7002,'upgrade',7001,repeat(chr(128512),25),'','Before upgrade');
    RAISE EXCEPTION 'Old schema unexpectedly accepts 100-byte names';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0021_public_name_limit.sql
"${runtime[@]}" <<'SQL'
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7002,'upgrade',7001,repeat(chr(128512),25),repeat('s',100),'After upgrade');
UPDATE content.posts SET deleted=true WHERE id=7001;
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7001 AND deleted AND name=repeat('n',80) AND subject=repeat('s',120) AND comment='Preserved old fields')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7002 AND octet_length(name)=100 AND char_length(name)=25)
  THEN RAISE EXCEPTION 'Historical values or runtime insertion were not preserved'; END IF;
  BEGIN
    INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
    VALUES(7003,'upgrade',7001,repeat('n',101),'','Too long');
    RAISE EXCEPTION 'Runtime accepted a name over the storage limit';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    ALTER TABLE content.posts DROP CONSTRAINT posts_name_check;
    RAISE EXCEPTION 'Runtime can change the field constraint';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Field migration passed: historical text and deletion preserved; 100-byte UTF-8 runtime name accepted; 101-byte name and runtime schema mutation denied. Disposable database removed.'
