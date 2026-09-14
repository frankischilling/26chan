#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the disposable development cluster.' >&2; exit 1; }
source .local/database.env
source .local/staff.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p 55432 -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'Port 55432 belongs to a different cluster.' >&2; exit 1; }
upgrade_db="imageboard_bump_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_bump_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
GRANT CONNECT ON DATABASE :"upgrade_db" TO board_migrator,board_public,board_staff;
SQL
created=1
upgrade_url="${MIGRATION_DATABASE_URL%/imageboard}/$upgrade_db"
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0022_thread_bump_flags.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Bump upgrade','Owned synthetic upgrade fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board,bumped_at,reply_count,undead) VALUES(7001,'upgrade','2026-01-01T00:00:00Z',1,true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7001,'upgrade',7001,'Anonymous','Preserved subject','Preserved old root'),
      (7002,'upgrade',7001,'Anonymous','','Preserved old reply');
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'upgrade',7001,'close');
SQL
"${db[@]}" --single-transaction -f migrations/0022_thread_bump_flags.sql
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_public' OR NOT EXISTS(
      SELECT 1 FROM content.visible_threads WHERE id=7001 AND NOT permasage AND NOT permaage
        AND undead AND reply_count=1 AND bumped_at='2026-01-01T00:00:00Z')
    OR (SELECT count(*) FROM content.posts WHERE board='upgrade') <> 2
  THEN RAISE EXCEPTION 'Historical content or false defaults were not preserved'; END IF;
  BEGIN
    UPDATE content.threads SET permasage=true WHERE id=7001;
    RAISE EXCEPTION 'Public runtime can change permasage';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.threads SET permaage=true WHERE id=7001;
    RAISE EXCEPTION 'Public runtime can change permaage';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.threads(id,board,permaage) VALUES(7003,'upgrade',true);
    RAISE EXCEPTION 'Public runtime can insert permaage';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.threads(id,board,permasage) VALUES(7003,'upgrade',true);
    RAISE EXCEPTION 'Public runtime can insert permasage';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.threads DROP COLUMN permaage;
    RAISE EXCEPTION 'Public runtime can change the schema';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
staff_url="${STAFF_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$staff_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_staff' THEN RAISE EXCEPTION 'Wrong staff runtime identity'; END IF;
END $$;
BEGIN;
UPDATE content.threads SET permasage=true,permaage=true WHERE id=7001;
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'upgrade',7001,'permasage'),(42,'upgrade',7001,'permaage');
COMMIT;
BEGIN;
UPDATE content.threads SET permasage=false,permaage=false WHERE id=7001;
INSERT INTO content.moderation_audit(account_id,board,target_id,action)
VALUES(42,'upgrade',7001,'unpermasage'),(42,'upgrade',7001,'unpermaage');
ROLLBACK;
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.visible_threads WHERE id=7001 AND permasage AND permaage
      AND bumped_at='2026-01-01T00:00:00Z' AND reply_count=1)
  THEN RAISE EXCEPTION 'Flag update, rollback or public projection failed'; END IF;
END $$;
SQL
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT array_agg(action ORDER BY id) FROM content.moderation_audit WHERE board='upgrade')
      IS DISTINCT FROM ARRAY['close','permasage','permaage']::text[]
    OR NOT EXISTS(SELECT 1 FROM pg_class WHERE oid='content.visible_threads'::regclass
                  AND reloptions @> ARRAY['security_barrier=true'])
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7001 AND subject='Preserved subject' AND comment='Preserved old root')
  THEN RAISE EXCEPTION 'Audit, visibility barrier or historical text changed'; END IF;
END $$;
SQL
cleanup
echo 'Bump flag upgrade passed: historical content retained, defaults false, public flag/schema writes denied, staff updates audited, rollback atomic and visible view barrier retained. Disposable database removed.'
