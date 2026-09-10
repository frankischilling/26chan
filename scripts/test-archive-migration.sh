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
[[ $actual = "$cluster" ]] || { echo 'Expected owned disposable cluster.' >&2; exit 1; }
upgrade_db="imageboard_archive_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_archive_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
SQL
created=1
"${admin[@]}" -v upgrade_db="$upgrade_db" <<'SQL'
REVOKE ALL ON DATABASE :"upgrade_db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public;
SQL
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/000{1,2,3,4,5,6,7,8}_*.sql; do
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES ('upgrade','Archive upgrade','Synthetic fixture',100,20,10,3,1);
INSERT INTO content.threads(id,board,sticky,closed,deleted) VALUES
 (7001,'upgrade',false,false,false), (7002,'upgrade',true,true,false), (7003,'upgrade',false,false,true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES (7001,'upgrade',7001,'Anonymous','Before archive support','Preserve this synthetic text');
CREATE TABLE public.archive_upgrade_baseline AS SELECT to_jsonb(t) AS row FROM content.threads t;
SQL
"${db[@]}" --single-transaction -f migrations/0009_thread_archives.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.threads)<>3 OR EXISTS (
    SELECT 1 FROM content.threads t FULL JOIN public.archive_upgrade_baseline b
      ON (b.row->>'id')::bigint=t.id
    WHERE to_jsonb(t)-'archived_at'-'archive_expires_at' IS DISTINCT FROM b.row
  ) THEN RAISE EXCEPTION 'Historical thread metadata changed'; END IF;
  IF EXISTS(SELECT 1 FROM content.threads WHERE archived_at IS NOT NULL OR archive_expires_at IS NOT NULL)
    OR (SELECT archive_retention_seconds<>0 OR archive_limit<>1000 FROM content.boards WHERE slug='upgrade')
    OR (SELECT comment<>'Preserve this synthetic text' FROM content.posts WHERE id=7001)
  THEN RAISE EXCEPTION 'Archive migration changed existing policy or text'; END IF;
  IF has_column_privilege('board_public','content.threads','closed','UPDATE')
    OR has_column_privilege('board_public','content.threads','sticky','UPDATE')
    OR has_column_privilege('board_public','content.boards','archive_retention_seconds','UPDATE')
    OR has_column_privilege('board_staff','content.threads','archived_at','UPDATE')
    OR has_table_privilege('board_media_read','content.visible_threads','SELECT')
  THEN RAISE EXCEPTION 'Archive migration expanded unrelated authority'; END IF;
END $$;
UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug='upgrade';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=statement_timestamp()+interval '1 hour' WHERE id=7001;
DO $$ BEGIN
  IF (SELECT count(*) FROM content.visible_threads)<>2 OR
    NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7001 AND archived_at IS NOT NULL)
  THEN RAISE EXCEPTION 'Real public login cannot read approved visibility'; END IF;
  BEGIN
    UPDATE content.threads SET closed=false WHERE id=7002;
    RAISE EXCEPTION 'Public login changed staff closure';
  EXCEPTION WHEN insufficient_privilege THEN NULL;
  END;
  BEGIN
    UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=statement_timestamp()+interval '1 hour' WHERE id=7002;
    RAISE EXCEPTION 'Pinned thread became archived';
  EXCEPTION WHEN check_violation THEN NULL;
  END;
END $$;
UPDATE content.threads SET archived_at=statement_timestamp()-interval '2 hours',archive_expires_at=statement_timestamp()-interval '1 hour' WHERE id=7001;
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7001)
  THEN RAISE EXCEPTION 'Expired archive still visible'; END IF;
END $$;
SQL
cleanup
trap - EXIT
printf 'Archive migration passed: 0008 metadata/text preserved, archives disabled by default, real public visibility and denials verified; owned database removed.\n'
