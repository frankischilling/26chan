#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run against the owned disposable development cluster as root.' >&2; exit 1; }
source .local/database.env
cluster=$(cat .local/cluster-path)
[[ $cluster =~ ^/tmp/board-postgres\.[[:alnum:]]+$ && -d $cluster ]] || exit 1
port=${BOARD_TEST_PORT:-55432}
[[ $port =~ ^[0-9]{1,5}$ && $port -gt 0 && $port -lt 65536 ]] || exit 1
pg_bin=/usr/lib/postgresql/16/bin
admin=(runuser -u postgres -- "$pg_bin/psql" -Xq -v ON_ERROR_STOP=1 -h /tmp -p "$port" -d postgres)
actual=$("${admin[@]}" -At -c 'SHOW data_directory')
[[ $actual = "$cluster" ]] || { echo 'The selected port belongs to another cluster.' >&2; exit 1; }
upgrade_db="imageboard_trip_policy_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_trip_policy_[0-9]+_[0-9]+$ ]] || exit 1
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
export OWNED_TRIP_UPGRADE_DATABASE="$upgrade_db"
upgrade_url=$(python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ['MIGRATION_DATABASE_URL'])
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_TRIP_UPGRADE_DATABASE'])))
PY
)
public_url=$(python3 - <<'PY'
import os, urllib.parse
value = urllib.parse.urlsplit(os.environ['TEST_PUBLIC_DATABASE_URL'])
print(urllib.parse.urlunsplit(value._replace(path='/' + os.environ['OWNED_TRIP_UPGRADE_DATABASE'])))
PY
)
db=("$pg_bin/psql" "$upgrade_url" -Xq -v ON_ERROR_STOP=1)
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
for migration in migrations/*.sql; do
  [[ $migration != migrations/0068_trip_suppression.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
UPDATE content.boards SET word_filter_enabled=false WHERE slug IN ('b','s4s','g');
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(7861,'b','2026-01-01Z','2026-01-02Z'),
      (7862,'s4s','2026-01-01Z','2026-01-02Z'),
      (7863,'g','2026-01-01Z','2026-01-02Z');
SQL
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7861,'b',7861,'Historical#literal','Owned history','Owned history','2026-01-01Z'),
      (7862,'s4s',7862,'Historical#literal','Owned history','Owned history','2026-01-01Z'),
      (7863,'g',7863,'Historical#literal','Owned history','Owned history','2026-01-01Z');
COMMIT;
DO $$ BEGIN
  IF (SELECT count(*) FROM content.posts WHERE trip='!ozOtJW9BFA')<>3 THEN
    RAISE EXCEPTION 'Unsuppressed historical control did not save trips'; END IF;
END $$;
SQL
"${db[@]}" <<'SQL'
CREATE TABLE public.owned_trip_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_trip_threads_before AS SELECT * FROM content.threads;
CREATE TABLE public.owned_trip_boards_before AS SELECT to_jsonb(b) AS value FROM content.boards b;
CREATE TABLE public.owned_trip_function_before AS
SELECT oid,proowner,prosecdef,proacl::text AS acl,proconfig
FROM pg_proc WHERE oid='content.apply_post_trip()'::regprocedure;
CREATE TABLE public.owned_trip_acls_before AS
SELECT relacl::text AS acl FROM pg_class WHERE oid='content.posts'::regclass;
SQL
"${db[@]}" --single-transaction -f migrations/0068_trip_suppression.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards)<>82
    OR EXISTS(SELECT 1 FROM content.boards WHERE strip_tripcode IS DISTINCT FROM (slug IN ('b','s4s')))
  THEN RAISE EXCEPTION 'Trip suppression defaults differ from source'; END IF;
  IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_trip_posts_before)
    OR EXISTS(SELECT * FROM public.owned_trip_posts_before EXCEPT SELECT * FROM content.posts)
    OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_trip_threads_before)
    OR EXISTS(SELECT * FROM public.owned_trip_threads_before EXCEPT SELECT * FROM content.threads)
    OR EXISTS(SELECT to_jsonb(b)-'strip_tripcode' FROM content.boards b EXCEPT SELECT value FROM public.owned_trip_boards_before)
    OR EXISTS(SELECT value FROM public.owned_trip_boards_before EXCEPT SELECT to_jsonb(b)-'strip_tripcode' FROM content.boards b)
  THEN RAISE EXCEPTION 'Trip policy migration changed history, clocks or unrelated board settings'; END IF;
  IF EXISTS(SELECT oid,proowner,prosecdef,proacl::text,proconfig FROM pg_proc
      WHERE oid='content.apply_post_trip()'::regprocedure EXCEPT SELECT * FROM public.owned_trip_function_before)
    OR EXISTS(SELECT relacl::text FROM pg_class WHERE oid='content.posts'::regclass
      EXCEPT SELECT * FROM public.owned_trip_acls_before)
    OR has_function_privilege('board_public','content.apply_post_trip()','EXECUTE')
    OR has_column_privilege('board_public','content.posts','trip','INSERT,UPDATE')
    OR has_column_privilege('board_public','content.boards','strip_tripcode','UPDATE')
    OR NOT has_column_privilege('board_attachment_owner','content.boards','strip_tripcode','SELECT')
    OR has_column_privilege('board_attachment_owner','content.boards','strip_tripcode','UPDATE')
  THEN RAISE EXCEPTION 'Trip policy migration changed identity or policy authority'; END IF;
END $$;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('future','Owned future board','Owned fixture',1000,100,100,100,10);
DO $$ BEGIN
  IF (SELECT strip_tripcode FROM content.boards WHERE slug='future') IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'Future board trip policy differs from global default'; END IF;
END $$;
SQL
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7864,'b',7861,'User','','Owned suppressed normal'),
      (7865,'s4s',7862,'','','Owned suppressed trip-only'),
      (7866,'g',7863,'','','Owned ordinary trip-only');
COMMIT;
BEGIN;
SELECT set_config('board.post_trip','!!XYWOFgjf7hP',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7867,'b',7861,'User','','Owned suppressed secure');
COMMIT;
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7864 AND name='User' AND trip IS NULL)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7865 AND name='Anonymous' AND trip IS NULL)
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7866 AND name='' AND trip='!ozOtJW9BFA')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7867 AND name='User' AND trip IS NULL)
  THEN RAISE EXCEPTION 'Trip policy trigger lost normal, secure or empty-name behavior'; END IF;
  BEGIN
    UPDATE content.boards SET strip_tripcode=false WHERE slug='b';
    RAISE EXCEPTION 'Public changed trip policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.posts SET trip=NULL WHERE id=7861;
    RAISE EXCEPTION 'Public changed a historical trip' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${db[@]}" -c "UPDATE content.boards SET strip_tripcode=false WHERE slug='b';"
"${runtime[@]}" <<'SQL'
BEGIN;
SELECT set_config('board.post_trip','!ozOtJW9BFA',true);
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
VALUES(7868,'b',7861,'User','','Owned restored trip');
COMMIT;
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7868 AND trip='!ozOtJW9BFA')
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7864 AND trip IS NULL)
    OR (SELECT count(*) FROM content.posts WHERE id IN (7861,7862,7863) AND trip='!ozOtJW9BFA')<>3
  THEN RAISE EXCEPTION 'Trip policy toggle rewrote earlier identities'; END IF;
END $$;
SQL
cleanup
echo 'Trip suppression upgrade passed: all 82 defaults, historical identities/clocks/settings/grants, future defaults, normal/secure/trip-only inserts, toggles and actual role denials.'
