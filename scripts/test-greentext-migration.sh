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
upgrade_db="imageboard_greentext_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_greentext_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0175_source_greentext.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" -f fixtures/demo.sql
"${db[@]}" <<'SQL'
-- Keep both source policy extremes represented; the demo intentionally uses profile 0.
UPDATE content.posts SET comment_format=CASE id WHEN 1000001 THEN 104 ELSE 127 END
 WHERE board='demo' AND id IN(1000001,1000002);
INSERT INTO content.moderation_audit(account_id,board,target_id,action,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler)
SELECT 4242,board,id,'spoiler',1,name,subject,comment,comment_format,false,false,false
 FROM content.posts WHERE board='demo' AND id IN(1000001,1000002) ORDER BY id;
CREATE TABLE public.owned_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_audits_before AS SELECT * FROM content.moderation_audit;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE comment_format BETWEEN 104 AND 127) THEN
  RAISE EXCEPTION 'Populated predecessor fixture is missing'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.moderation_audit WHERE snapshot_comment_format BETWEEN 104 AND 127) THEN
  RAISE EXCEPTION 'Historical audit snapshot fixture is missing'; END IF;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0175_source_greentext.sql
"${db[@]}" -f scripts/test-greentext-upgrade.sql
printf 'Greentext upgrade preserves saved rows and qualifies every new profile.\n'
