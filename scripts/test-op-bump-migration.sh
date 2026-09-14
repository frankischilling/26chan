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
upgrade_db="imageboard_op_upgrade_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_op_upgrade_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0024_op_self_bumps.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('upgrade','OP upgrade','Owned historical fixture',4000,100,100,100,10);
INSERT INTO content.threads(id,board,bumped_at,reply_count) VALUES(7201,'upgrade','2026-01-01T00:00:00Z',2);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7201,'upgrade',7201,'Anonymous','Retained subject','Retained OP'),
      (7202,'upgrade',7201,'Anonymous','','Retained first reply'),
      (7203,'upgrade',7201,'Anonymous','','Retained second reply');
SQL
"${db[@]}" --single-transaction -f migrations/0024_op_self_bumps.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='upgrade' AND op_bump_limit AND op_bump_initial_seconds=900 AND op_bump_repeat_seconds=300)
    OR EXISTS(SELECT 1 FROM post_secrets.op_peers)
    OR EXISTS(SELECT 1 FROM post_secrets.op_replies)
    OR (SELECT count(*) FROM content.posts WHERE board='upgrade') <> 3
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7201 AND subject='Retained subject' AND comment='Retained OP')
    OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7201 AND reply_count=2 AND bumped_at='2026-01-01T00:00:00Z')
  THEN RAISE EXCEPTION 'Historical content/defaults changed or old peer identity invented'; END IF;
  BEGIN
    UPDATE content.boards SET op_bump_initial_seconds=-1 WHERE slug='upgrade';
    RAISE EXCEPTION 'Negative initial interval accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
  BEGIN
    UPDATE content.boards SET op_bump_repeat_seconds=-1 WHERE slug='upgrade';
    RAISE EXCEPTION 'Negative repeat interval accepted';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
UPDATE content.boards SET op_bump_initial_seconds=0,op_bump_repeat_seconds=2147483647 WHERE slug='upgrade';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_public' THEN RAISE EXCEPTION 'Wrong public identity'; END IF;
  BEGIN
    UPDATE content.boards SET op_bump_limit=false,op_bump_initial_seconds=0,op_bump_repeat_seconds=0 WHERE slug='upgrade';
    RAISE EXCEPTION 'Public policy update accepted';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    ALTER TABLE post_secrets.op_peers DROP COLUMN peer;
    RAISE EXCEPTION 'Public schema change accepted';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(7201,'192.0.2.0/24');
    RAISE EXCEPTION 'Address range accepted as a peer';
  EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(7201,'192.0.2.1');
INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES(7202,7201),(7203,7201);
BEGIN;
UPDATE content.posts SET deleted=true WHERE id=7203;
ROLLBACK;
DO $$ BEGIN
  IF (SELECT count(*) FROM post_secrets.op_replies WHERE thread_id=7201) <> 2
  THEN RAISE EXCEPTION 'Reply cleanup did not roll back'; END IF;
END $$;
UPDATE content.posts SET deleted=true WHERE id=7203;
DO $$ BEGIN
  IF (SELECT count(*) FROM post_secrets.op_replies WHERE thread_id=7201) <> 1
  THEN RAISE EXCEPTION 'Reply deletion did not clean membership'; END IF;
END $$;
SQL
# Actual staff/media logins can connect but cannot read private OP records.
for runtime_url in "${STAFF_DATABASE_URL%/imageboard}/$upgrade_db" "${MEDIA_DATABASE_URL%/imageboard}/$upgrade_db"; do
  "$pg_bin/psql" "$runtime_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user NOT IN ('board_staff','board_media') THEN RAISE EXCEPTION 'Wrong runtime identity'; END IF;
  BEGIN
    PERFORM * FROM post_secrets.op_peers;
    RAISE EXCEPTION 'Runtime can read OP addresses';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    PERFORM * FROM post_secrets.op_replies;
    RAISE EXCEPTION 'Runtime can read own reply membership';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
done
staff_url="${STAFF_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$staff_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;
SELECT slug FROM content.boards WHERE slug='upgrade' FOR UPDATE;
UPDATE content.threads SET deleted=true WHERE id=7201;
ROLLBACK;
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=7201)
  THEN RAISE EXCEPTION 'Staff rollback lost OP identity'; END IF;
END $$;
SQL
"$pg_bin/psql" "$staff_url" -Xq -v ON_ERROR_STOP=1 -c 'UPDATE content.threads SET deleted=true WHERE id=7201'
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM post_secrets.op_peers) OR EXISTS(SELECT 1 FROM post_secrets.op_replies)
  THEN RAISE EXCEPTION 'Staff deletion left private posting data'; END IF;
END $$;
-- Exercise the same cleanup on archival, with original content retained.
UPDATE content.threads SET deleted=false WHERE id=7201;
INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES(7201,'2001:db8::1');
UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=7201;
DO $$ BEGIN
  IF EXISTS(SELECT 1 FROM post_secrets.op_peers)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7201 AND subject='Retained subject')
  THEN RAISE EXCEPTION 'Archive cleanup or content retention failed'; END IF;
END $$;
SQL
cleanup
echo 'OP policy upgrade passed: history/defaults retained without invented identity; bounded policies, actual public writes, staff/media denials, transactional deletion and archive cleanup verified. Disposable database removed.'
