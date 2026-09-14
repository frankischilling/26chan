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
upgrade_db="imageboard_image_admission_$(date +%s)_$RANDOM"
[[ $upgrade_db =~ ^imageboard_image_admission_[0-9]+_[0-9]+$ ]] || exit 1
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
  [[ $migration != migrations/0026_image_admission_flags.sql ]] || break
  "${db[@]}" --single-transaction -f "$migration"
done
"${db[@]}" <<'SQL'
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit)
VALUES('upgrade','Image admission','Owned fixture',4000,100,100,100,10,1);
INSERT INTO content.threads(id,board,sticky,reply_count,created_at,modified_at)
VALUES(7401,'upgrade',true,1,'2026-01-01Z','2026-01-02Z');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at)
VALUES(7401,'upgrade',7401,'Anonymous','Retained','Owned OP','2026-01-01Z'),
      (7402,'upgrade',7401,'Anonymous','','Owned first image','2026-01-02Z');
-- Harmless metadata fixtures, not decoder/publication evidence.
INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes)
SELECT md5('owned-image-'||i), 'owned.png','published',100,1,md5('owned-lease-'||i),repeat('a',64),123 FROM generate_series(1,5) i;
INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
SELECT id,id,lease_token,output_sha256,123,10,20,'approved',clock_timestamp() FROM media.jobs;
INSERT INTO media_intake.handles(job_id,capability_hash)
SELECT id,sha256(convert_to(repeat('c',64),'UTF8')) FROM media.jobs;
INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
VALUES(7402,md5('owned-image-1'),md5('owned-image-1'),'owned.png',123,10,20,false);
SQL
public_url="${TEST_PUBLIC_DATABASE_URL%/imageboard}/$upgrade_db"
runtime=("$pg_bin/psql" "$public_url" -Xq -v ON_ERROR_STOP=1)
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  IF current_user <> 'board_public' THEN RAISE EXCEPTION 'Wrong identity'; END IF;
  BEGIN
    PERFORM content.insert_post_attachment(7403,'upgrade',7401,'Anonymous','','Owned second',md5('owned-image-2'),repeat('c',64),false);
    RAISE EXCEPTION 'Old schema unexpectedly bypassed the cap' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0001' THEN NULL; END;
END $$;
SQL
"${db[@]}" --single-transaction -f migrations/0026_image_admission_flags.sql
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.posts) <> 2 OR (SELECT count(*) FROM content.post_media) <> 1
    OR NOT EXISTS(SELECT 1 FROM content.threads WHERE id=7401 AND sticky AND reply_count=1 AND created_at='2026-01-01Z' AND modified_at='2026-01-02Z')
    OR NOT has_column_privilege('board_attachment_owner','content.threads','sticky','SELECT')
    OR NOT has_column_privilege('board_attachment_owner','content.threads','undead','SELECT')
    OR has_column_privilege('board_attachment_owner','content.threads','sticky','UPDATE')
    OR has_column_privilege('board_attachment_owner','content.threads','undead','UPDATE')
    OR has_schema_privilege('board_attachment_owner','content','CREATE')
  THEN RAISE EXCEPTION 'Historical state or scoped grants changed'; END IF;
  IF (SELECT count(*) FROM pg_proc WHERE pronamespace='content'::regnamespace AND proname='insert_post_attachment'
      AND pronargs IN (9,10) AND prosecdef AND proowner='board_attachment_owner'::regrole
      AND proconfig @> ARRAY['search_path=pg_catalog, pg_temp']
      AND NOT EXISTS(SELECT 1 FROM aclexplode(proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE')) <> 2
  THEN RAISE EXCEPTION 'Attachment entry-point authority changed'; END IF;
END $$;
SQL
# The unchanged nine-argument wrapper now sees the corrected sticky rule.
"${runtime[@]}" -c "SELECT content.insert_post_attachment(7403,'upgrade',7401,'Anonymous','','Owned second',md5('owned-image-2'),repeat('c',64),false)"
"${db[@]}" -c "UPDATE content.threads SET sticky=false,undead=true WHERE id=7401"
"${runtime[@]}" -c "SELECT content.insert_post_attachment(7404,'upgrade',7401,'Anonymous','','Owned third',md5('owned-image-3'),repeat('c',64),false,'2026-02-01Z'::timestamptz)"
"${db[@]}" -c "UPDATE content.threads SET undead=false,permaage=true WHERE id=7401"
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    PERFORM content.insert_post_attachment(7405,'upgrade',7401,'Anonymous','','Blocked permaage',md5('owned-image-4'),repeat('c',64),false,clock_timestamp());
    RAISE EXCEPTION 'Permaage bypassed count admission' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0001' THEN NULL; END;
  BEGIN
    UPDATE content.threads SET sticky=true,undead=true WHERE id=7401;
    RAISE EXCEPTION 'Public changed admission flags' USING ERRCODE='ZX001';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SQL
"${db[@]}" -c "UPDATE content.threads SET sticky=true,undead=true,closed=true WHERE id=7401"
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    PERFORM content.insert_post_attachment(7405,'upgrade',7401,'Anonymous','','Closed',md5('owned-image-4'),repeat('c',64),false,clock_timestamp());
    RAISE EXCEPTION 'Flag bypassed closed state' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0002' THEN NULL; END;
END $$;
SQL
"${db[@]}" <<'SQL'
UPDATE content.threads SET closed=false WHERE id=7401;
UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=md5('owned-image-5');
SQL
"${runtime[@]}" <<'SQL'
DO $$ BEGIN
  BEGIN
    PERFORM content.insert_post_attachment(7405,'upgrade',7401,'Anonymous','','Expired',md5('owned-image-5'),repeat('c',64),false,clock_timestamp()-interval '4 hours');
    RAISE EXCEPTION 'Flag or old clock renewed a capability' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0002' THEN NULL; END;
  BEGIN
    PERFORM content.insert_post_attachment(7405,'upgrade',7401,'Anonymous','','Reused',md5('owned-image-3'),repeat('c',64),false,clock_timestamp());
    RAISE EXCEPTION 'Flag permitted capability reuse' USING ERRCODE='ZX001';
  EXCEPTION WHEN SQLSTATE 'P0001' THEN NULL; END;
END $$;
SQL
"${db[@]}" <<'SQL'
DO $$ BEGIN
  IF (SELECT count(*) FROM content.post_media) <> 3 OR (SELECT count(*) FROM content.posts) <> 4
    OR NOT EXISTS(SELECT 1 FROM content.posts WHERE id=7404 AND created_at='2026-02-01Z')
  THEN RAISE EXCEPTION 'Upgrade insertion or failure rollback mismatch'; END IF;
END $$;
SQL
cleanup
echo 'Image admission upgrade passed: historical rows retained, old/new entry points apply sticky/undead exceptions, scoped read-only flags, permaage cap, closed state, expiry and single-use retained. Disposable database removed.'
