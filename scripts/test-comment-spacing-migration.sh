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
upgrade_db="imageboard_spacing_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_spacing_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0027_comment_spacing.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
SELECT slug,'Spacing upgrade','Owned fixture',1000,100,100,10,10 FROM unnest(ARRAY['g','j','test','jp','vip','a','b','demo']) slug;
INSERT INTO content.threads(id,board,created_at,modified_at)
VALUES(7501,'demo','2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7501,'demo',7501,'Anonymous','Owned historical',E' historical\t  text\r\n\r\n\r\n\r\n ','2026-01-01Z');
SQL
"${db[@]}" --single-transaction -f migrations/0027_comment_spacing.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.boards WHERE comment_code_spacing) <> 3
    OR (SELECT count(*) FROM content.boards WHERE comment_sjis_spacing) <> 2
    OR EXISTS(SELECT 1 FROM content.boards WHERE comment_code_spacing IS DISTINCT FROM (slug IN ('g','j','test'))
          OR comment_sjis_spacing IS DISTINCT FROM (slug IN ('jp','vip')))
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7501 AND comment=E' historical\t  text\r\n\r\n\r\n\r\n ' AND created_at='2026-01-01Z')
    OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7501 AND modified_at='2026-01-02Z')
  THEN RAISE EXCEPTION 'Historical content or source spacing defaults changed'; END IF;
END $$;
-- A new board starts with the source global defaults; the operator selects any override.
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('new','New fixture','Owned fixture',1000,100,100,10,10);
DO $$ BEGIN
  IF NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='new' AND NOT comment_code_spacing AND NOT comment_sjis_spacing)
  THEN RAISE EXCEPTION 'New board did not use source global spacing defaults'; END IF;
END $$;
UPDATE content.boards SET comment_code_spacing=true,comment_sjis_spacing=true WHERE slug='new';
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
"$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1 <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_public' OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug='new' AND comment_code_spacing AND comment_sjis_spacing)
  THEN RAISE EXCEPTION 'Public login cannot read configured policy'; END IF;
  BEGIN
    UPDATE content.boards SET comment_code_spacing=false,comment_sjis_spacing=false WHERE slug='new';
    RAISE EXCEPTION 'Public changed source spacing policy' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  BEGIN
    UPDATE content.posts SET comment='rewritten' WHERE id=7501;
    RAISE EXCEPTION 'Public rewrote historical comments' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
cleanup
echo 'Comment spacing upgrade passed: historical text/clocks retained, source overrides configured, actual public read-only policy and immutable comments. Disposable database removed.'
