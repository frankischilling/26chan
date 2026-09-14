#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
source .local/staff.env
source .local/media.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_clock_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_clock_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public,board_staff,board_media;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0025_posting_timestamps.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Clock upgrade','Owned historical fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board,created_at,modified_at,bumped_at,reply_count)
VALUES(7301,'upgrade','2026-01-01T01:00:00.123Z','2026-01-01T03:00:00.456Z','2026-01-01T02:00:00.789Z',1);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7301,'upgrade',7301,'Anonymous','Retained subject','Retained OP','2026-01-01T01:00:00.123Z'),
      (7302,'upgrade',7301,'Anonymous','','Retained reply','2026-01-01T03:00:00.456Z');
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(7301,'192.0.2.1');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(7302,7301);
SQL
"${db[@]}" --single-transaction -f migrations/0025_posting_timestamps.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7301 AND created_at='2026-01-01T01:00:00.123Z'
      AND modified_at='2026-01-01T03:00:00.456Z' AND bumped_at='2026-01-01T02:00:00.789Z' AND reply_count=1
      AND http_modified_at BETWEEN clock_timestamp()-interval '5 minutes' AND clock_timestamp())
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7301 AND subject='Retained subject' AND created_at='2026-01-01T01:00:00.123Z')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7302 AND comment='Retained reply' AND created_at='2026-01-01T03:00:00.456Z')
    OR (SELECT count(*) FROM post_secrets.op_peers) <> 1 OR (SELECT count(*) FROM post_secrets.op_replies) <> 1
  THEN RAISE EXCEPTION 'Historical clocks, content or private state changed'; END IF;
  IF NOT EXISTS(SELECT 1 FROM pg_class WHERE oid='content.visible_threads'::regclass AND reloptions @> ARRAY['security_barrier=true'])
    OR has_function_privilege('board_public','content.advance_http_clock()','EXECUTE')
    OR has_schema_privilege('board_attachment_owner','content','CREATE')
  THEN RAISE EXCEPTION 'View barrier or runtime authority is wrong'; END IF;
  IF (SELECT count(*) FROM pg_proc WHERE pronamespace='content'::regnamespace AND proname='insert_post_attachment'
      AND pronargs IN (9,10) AND prosecdef AND proowner='board_attachment_owner'::regrole
      AND proconfig @> ARRAY['search_path=pg_catalog, pg_temp']
      AND NOT EXISTS(SELECT 1 FROM aclexplode(proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE')) <> 2
  THEN RAISE EXCEPTION 'Compatibility attachment entry points lack protected ownership/search path/ACL'; END IF;
END $$;
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$
DECLARE old_clock timestamptz; changed_clock timestamptz;
BEGIN
  IF current_user <> 'board_public' THEN RAISE EXCEPTION 'Wrong public identity'; END IF;
  SELECT http_modified_at INTO STRICT old_clock FROM content.visible_threads WHERE id=7301;
  UPDATE content.threads SET modified_at='2025-01-01Z' WHERE id=7301;
  SELECT http_modified_at INTO STRICT changed_clock FROM content.visible_threads WHERE id=7301;
  IF changed_clock <= old_clock OR NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7301 AND modified_at='2025-01-01Z')
  THEN RAISE EXCEPTION 'Source clock did not regress independently of HTTP change clock'; END IF;
  BEGIN
    UPDATE content.threads SET modified_at='2024-01-01Z' WHERE id=7301;
    RAISE EXCEPTION 'Owned rollback marker' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'ZX001' THEN NULL; END;
  IF NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7301 AND modified_at='2025-01-01Z' AND http_modified_at=changed_clock)
  THEN RAISE EXCEPTION 'Clock change did not roll back'; END IF;
  BEGIN
    UPDATE content.posts SET created_at=clock_timestamp() WHERE id=7301;
    RAISE EXCEPTION 'Public rewrote a historical post time';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.threads SET created_at=clock_timestamp() WHERE id=7301;
    RAISE EXCEPTION 'Public rewrote a historical thread time';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.threads SET http_modified_at=clock_timestamp() WHERE id=7301;
    RAISE EXCEPTION 'Public supplied the HTTP clock';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.threads(id,board,http_modified_at) VALUES(7399,'upgrade','2020-01-01Z');
    RAISE EXCEPTION 'Public inserted its own HTTP clock';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
BEGIN;
INSERT INTO content.threads(id,board,created_at,modified_at) VALUES(7303,'upgrade','2026-02-01Z','2026-02-01Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7303,'upgrade',7303,'Anonymous','','Owned new request','2026-02-01Z');
COMMIT;
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7303 AND created_at='2026-02-01Z' AND modified_at='2026-02-01Z')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7303 AND created_at='2026-02-01Z')
  THEN RAISE EXCEPTION 'Public insert clocks were not retained'; END IF;
END $$;
SQL
# The healthy staff login can still moderate and the invoker trigger updates its clock.
staff_url="${STAFF_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$staff_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_staff' THEN RAISE EXCEPTION 'Wrong staff identity'; END IF;
  UPDATE content.threads SET closed=true WHERE id=7301;
END $$;
SQL
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7301 AND closed AND modified_at='2025-01-01Z'
      AND http_modified_at >= clock_timestamp()-interval '5 minutes')
    OR (SELECT count(*) FROM content.posts WHERE board='upgrade') <> 3
  THEN RAISE EXCEPTION 'Staff change clock or retained content failed'; END IF;
END $$;
SQL
cleanup
echo 'Posting clock upgrade passed: historical values retained, public insert-only times, independent transactional HTTP clock, protected attachment entry points and actual staff writes. Disposable database removed.'
