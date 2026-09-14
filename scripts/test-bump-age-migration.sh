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
upgrade_db="imageboard_age_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_age_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0023_board_bump_age.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','Age upgrade','Owned historical fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board,bumped_at,reply_count,permaage) VALUES(7101,'upgrade','2026-01-01T00:00:00Z',1,true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7101,'upgrade',7101,'Anonymous','Retained subject','Retained OP','2025-01-01T00:00:00.9Z'),
      (7102,'upgrade',7101,'Anonymous','','Retained reply','2026-01-01T00:00:00Z');
INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,'upgrade',7101,'permaage');
SQL
"${db[@]}" --single-transaction -f migrations/0023_board_bump_age.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT permasage_hours FROM content.boards WHERE slug='upgrade') <> 0
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7101 AND subject='Retained subject' AND comment='Retained OP' AND created_at='2025-01-01T00:00:00.9Z')
    OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7101 AND permaage AND reply_count=1 AND bumped_at='2026-01-01T00:00:00Z')
    OR (SELECT count(*) FROM content.posts WHERE board='upgrade') <> 2
    OR (SELECT array_agg(action ORDER BY id) FROM content.moderation_audit WHERE board='upgrade') IS DISTINCT FROM ARRAY['permaage']::text[]
  THEN RAISE EXCEPTION 'Historical state or disabled default was not preserved'; END IF;
  BEGIN
    UPDATE content.boards SET permasage_hours=-1 WHERE slug='upgrade';
    RAISE EXCEPTION 'Negative age policy accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.boards SET permasage_hours=NULL WHERE slug='upgrade';
    RAISE EXCEPTION 'Null age policy accepted';
  EXCEPTION WHEN not_null_violation THEN NULL; END;
  BEGIN
    UPDATE content.boards SET permasage_hours=2147483648 WHERE slug='upgrade';
    RAISE EXCEPTION 'Out-of-range age policy accepted';
  EXCEPTION WHEN numeric_value_out_of_range THEN NULL; END;
END $$;
UPDATE content.boards SET permasage_hours=2147483647 WHERE slug='upgrade';
BEGIN;
UPDATE content.boards SET permasage_hours=48 WHERE slug='upgrade';
ROLLBACK;
SQL
for runtime_url in "${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db" "${STAFF_DATABASE_URL%/imageboard}/$upgrade_db"; do
  "$pg_bin/psql" "$runtime_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user NOT IN ('board_public','board_staff')
    OR (SELECT permasage_hours FROM content.boards WHERE slug='upgrade') <> 2147483647
  THEN RAISE EXCEPTION 'Wrong identity or committed policy unreadable'; END IF;
  BEGIN
    UPDATE content.boards SET permasage_hours=0 WHERE slug='upgrade';
    RAISE EXCEPTION 'Runtime can change age policy';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO content.boards(slug,permasage_hours) VALUES('forged',48);
    RAISE EXCEPTION 'Runtime can insert age policy';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE content.boards DROP COLUMN permasage_hours;
    RAISE EXCEPTION 'Runtime can alter policy schema';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
-- Both identities retain their healthy board-row serialization permission.
BEGIN;
SELECT slug FROM content.boards WHERE slug='upgrade' FOR UPDATE;
ROLLBACK;
SQL
done
"${db[@]}" <<'SQL'
UPDATE content.boards SET permasage_hours=48 WHERE slug='upgrade';
DO $$ BEGIN
  IF (SELECT permasage_hours FROM content.boards WHERE slug='upgrade') <> 48
    OR NOT EXISTS(SELECT 1 FROM pg_class WHERE oid='content.visible_threads'::regclass AND reloptions @> ARRAY['security_barrier=true'])
  THEN RAISE EXCEPTION 'Operator policy update or visibility barrier failed'; END IF;
END $$;
SQL
cleanup
echo 'Age policy upgrade passed: history and disabled defaults retained, bounded operator updates and rollback verified, actual runtime policy/schema writes denied with healthy read and lock controls. Disposable database removed.'
